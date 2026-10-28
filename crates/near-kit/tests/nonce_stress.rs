//! Concurrent sends, `sign()` and `sign_delegate()` on one access key,
//! against in-process nodes that check nonces as nearcore does.
//!
//! A test binary of its own: the nonce cache is process-wide, and these
//! chains reuse one account, key and chain id on purpose.

#![cfg(feature = "rpc")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use near_kit::protocol::{Action, SignedTransaction};
use near_kit::rpc::{BoxFuture, RpcError, RpcTransport, TransportResponse};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::{DelegateOptions, Submitted};
use near_kit::{AccountId, CryptoHash, Near, NearToken, SigningNear, account};
use serde_json::{Value, json};

/// nearcore's `ACCESS_KEY_NONCE_RANGE_MULTIPLIER`.
const MULTIPLIER: u64 = 1_000_000;
/// Final views trail the optimistic head by this many blocks.
const FINAL_LAG: u64 = 2;
/// A block is produced every this many requests to a chain.
const REQUESTS_PER_BLOCK: u64 = 128;
const TASKS: usize = 32;
const OPS_PER_TASK: usize = 12;
/// Re-add the key once this many transactions landed on a chain.
const READD_AFTER: usize = 40;
/// The default: concurrent senders may overtake each other across a block
/// boundary, and every send has to land within it anyway.
const MAX_NONCE_RETRIES: u32 = 3;

/// One chain's state, changed one request at a time.
struct ChainState {
    height: u64,
    /// `(height, nonce)` each time the key's nonce changed, oldest first.
    history: Vec<(u64, u64)>,
    /// Accepted transactions' nonces, waiting for the next block.
    pending: Vec<u64>,
    requests: u64,
    landed: Vec<u64>,
    readded: bool,
    too_large: usize,
    /// Accepted, then not applied: the key's nonce had passed it.
    dropped: usize,
    /// Every nonce a client drew: send attempts, signatures, delegates.
    drawn: Vec<u64>,
}

impl ChainState {
    fn nonce_at(&self, height: u64) -> u64 {
        self.history
            .iter()
            .rev()
            .find(|&&(at, _)| at <= height)
            .map_or(self.history[0].1, |&(_, nonce)| nonce)
    }

    /// Apply the pending transactions in nonce order, as a chunk does.
    fn produce_block(&mut self) {
        self.height += 1;
        let mut nonce = self.nonce_at(self.height);
        let mut pending = std::mem::take(&mut self.pending);
        pending.sort_unstable();
        for tx_nonce in pending {
            if tx_nonce > nonce {
                nonce = tx_nonce;
                self.landed.push(tx_nonce);
            } else {
                self.dropped += 1;
            }
        }
        if !self.readded && self.landed.len() >= READD_AFTER {
            // Delete the key and add it back, after this block's
            // transactions: it starts at nearcore's `(height - 1) * 1e6`.
            // Final views show the old key for two more blocks.
            self.readded = true;
            nonce = (self.height - 1) * MULTIPLIER;
        }
        let height = self.height;
        self.history.push((height, nonce));
    }

    fn view_access_key(&self, finality: &str) -> Value {
        let height = match finality {
            "final" => self.height - FINAL_LAG,
            _ => self.height,
        };
        json!({
            "nonce": self.nonce_at(height),
            "permission": "FullAccess",
            "block_height": height,
            "block_hash": CryptoHash::hash(&height.to_le_bytes()),
        })
    }

    /// nearcore's checks on arrival, against the head: above the key's
    /// nonce, and below `height * 1e6` for the block that includes it.
    fn send_tx(&mut self, nonce: u64) -> Value {
        self.drawn.push(nonce);
        let ak_nonce = self.nonce_at(self.height);
        let upper_bound = (self.height + 1) * MULTIPLIER;
        if nonce <= ak_nonce {
            return invalid_tx(
                json!({ "InvalidNonce": { "tx_nonce": nonce, "ak_nonce": ak_nonce } }),
            );
        }
        if nonce >= upper_bound {
            self.too_large += 1;
            return invalid_tx(json!({
                "NonceTooLarge": { "tx_nonce": nonce, "upper_bound": upper_bound }
            }));
        }
        self.pending.push(nonce);
        json!({ "result": { "final_execution_status": "NONE" } })
    }

    fn status(&self, genesis: CryptoHash) -> Value {
        json!({
            "result": {
                "protocol_version": 83,
                "latest_protocol_version": 83,
                "chain_id": "sandbox",
                "genesis_hash": genesis,
                "sync_info": {
                    "latest_block_hash": CryptoHash::hash(&self.height.to_le_bytes()),
                    "latest_block_height": self.height,
                    "latest_block_time": "2026-01-01T00:00:00Z",
                    "syncing": false,
                },
                "version": { "version": "2.13.0", "build": "mock" },
            }
        })
    }
}

/// A chain: genesis hash and the state its nodes serve.
struct Chain {
    genesis: CryptoHash,
    state: Mutex<ChainState>,
}

impl Chain {
    /// A chain at `height` where the key was added a block earlier.
    fn new(seed: u8, height: u64) -> Arc<Self> {
        Arc::new(Self {
            genesis: CryptoHash::from_bytes([seed; 32]),
            state: Mutex::new(ChainState {
                height,
                history: vec![(height - 1, (height - 2) * MULTIPLIER)],
                pending: Vec::new(),
                requests: 0,
                landed: Vec::new(),
                readded: false,
                too_large: 0,
                dropped: 0,
                drawn: Vec::new(),
            }),
        })
    }

    /// One request, atomically.
    fn handle(&self, request: &Value) -> Value {
        let mut state = self.state.lock().unwrap();
        state.requests += 1;
        if state.requests.is_multiple_of(REQUESTS_PER_BLOCK) {
            state.produce_block();
        }
        match request["method"].as_str().unwrap() {
            "EXPERIMENTAL_view_access_key" => json!({
                "result": state.view_access_key(request["params"]["finality"].as_str().unwrap())
            }),
            "send_tx" => {
                let signed = request["params"]["signed_tx_base64"].as_str().unwrap();
                let nonce = SignedTransaction::from_base64(signed)
                    .unwrap()
                    .transaction
                    .nonce;
                state.send_tx(nonce)
            }
            "status" => state.status(self.genesis),
            other => panic!("unexpected RPC method {other}"),
        }
    }
}

fn invalid_tx(error: Value) -> Value {
    json!({
        "error": {
            "name": "HANDLER_ERROR",
            "cause": { "name": "INVALID_TRANSACTION", "info": {} },
            "code": -32000,
            "message": "Server error",
            "data": { "TxExecutionError": { "InvalidTxError": error } },
        }
    })
}

/// A node of `chain`, reached at one URL.
struct Node {
    chain: Arc<Chain>,
    jitter: AtomicU64,
}

impl RpcTransport for Node {
    fn post_json(
        &self,
        _url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        // A few hundred microseconds of latency, varying per request, so
        // concurrent requests arrive out of order.
        let delay = Duration::from_micros(self.jitter.fetch_add(137, Ordering::Relaxed) % 400);
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            let request: Value = serde_json::from_slice(&body).unwrap();
            let mut response = self.chain.handle(&request);
            response["jsonrpc"] = "2.0".into();
            response["id"] = 0.into();
            let body = serde_json::to_vec(&response).unwrap();
            Ok(TransportResponse { status: 200, body })
        })
    }
}

fn client(chain: &Arc<Chain>, url: &str, account_id: &AccountId, key: &SecretKey) -> SigningNear {
    Near::custom(url, "sandbox")
        .transport(Node {
            chain: chain.clone(),
            jitter: AtomicU64::new(url.len() as u64),
        })
        .max_nonce_retries(MAX_NONCE_RETRIES)
        .build()
        .with_signer(InMemorySigner::new(account_id.clone(), key.clone()))
}

/// Runs `TASKS` tasks over `clients`, each mixing sends, signatures and
/// delegate actions, and checks every chain's nonces afterwards.
async fn run(chains: &[Arc<Chain>], clients: Vec<(usize, SigningNear)>) {
    let clients = Arc::new(clients);
    let mut tasks = Vec::new();
    for task in 0..TASKS {
        let clients = clients.clone();
        tasks.push(tokio::spawn(async move {
            let mut drawn = Vec::new();
            for op in 0..OPS_PER_TASK {
                let (chain, near) = &clients[(task + op) % clients.len()];
                let transfer = vec![Action::transfer(NearToken::from_yoctonear(1))];
                match (task * 7 + op) % 4 {
                    0 => {
                        let signed = near.sign(account!("bob.sandbox"), transfer).await;
                        let signed = signed.unwrap_or_else(|e| panic!("sign: {e}"));
                        drawn.push((*chain, signed.transaction.nonce));
                    }
                    1 => {
                        let options = DelegateOptions {
                            max_block_height: Some(u64::MAX),
                            ..Default::default()
                        };
                        let signed = near
                            .sign_delegate(account!("bob.sandbox"), transfer, options)
                            .await
                            .unwrap_or_else(|e| panic!("sign_delegate: {e}"));
                        drawn.push((*chain, signed.delegate_action.nonce));
                    }
                    _ => {
                        near.wait_until::<Submitted>()
                            .transaction(account!("bob.sandbox"))
                            .add_action(Action::transfer(NearToken::from_yoctonear(1)))
                            .await
                            .unwrap_or_else(|e| {
                                panic!("send within {MAX_NONCE_RETRIES} retries: {e}")
                            });
                    }
                }
            }
            drawn
        }));
    }
    let mut offline: HashMap<usize, Vec<u64>> = HashMap::new();
    for task in tasks {
        for (chain, nonce) in task.await.unwrap() {
            offline.entry(chain).or_default().push(nonce);
        }
    }

    for (index, chain) in chains.iter().enumerate() {
        let mut state = chain.state.lock().unwrap();
        state.produce_block();
        assert_eq!(state.too_large, 0, "chain {index}: a nonce past its bound");
        assert_eq!(
            state.dropped, 0,
            "chain {index}: an accepted nonce never ran"
        );
        assert!(state.readded, "chain {index} re-added its key");
        let mut seen = HashSet::new();
        for nonce in state
            .drawn
            .iter()
            .chain(offline.get(&index).into_iter().flatten())
        {
            assert!(
                seen.insert(*nonce),
                "chain {index}: nonce {nonce} drawn twice"
            );
        }
        assert!(
            state.landed.windows(2).all(|w| w[0] < w[1]),
            "chain {index}: nonces land increasing"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_chain_over_two_url_spellings() {
    let account_id = account!("alice.sandbox");
    let key = SecretKey::generate_ed25519();
    let chain = Chain::new(1, 1095);
    // Several clients per spelling: each fetches the genesis hash itself.
    let clients = (0..4)
        .map(|i| {
            let url = if i % 2 == 0 {
                "http://localhost:3030"
            } else {
                "http://127.0.0.1:3030"
            };
            (0, client(&chain, url, &account_id, &key))
        })
        .collect();
    run(&[chain], clients).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_chains_sharing_a_chain_id() {
    // Two sandboxes: one chain id and key, the short chain's nonces far
    // below the tall one's bound and the tall one's far above the short's.
    let account_id = account!("carol.sandbox");
    let key = SecretKey::generate_ed25519();
    let short = Chain::new(2, 122);
    let tall = Chain::new(3, 1095);
    let clients = (0..4)
        .map(|i| {
            if i % 2 == 0 {
                (
                    0,
                    client(&short, "http://127.0.0.1:3030", &account_id, &key),
                )
            } else {
                (1, client(&tall, "http://127.0.0.1:3031", &account_id, &key))
            }
        })
        .collect();
    run(&[short, tall], clients).await;
}
