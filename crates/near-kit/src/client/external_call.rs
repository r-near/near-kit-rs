//! Sending, dry-running and finding external calls.

use std::future::IntoFuture;
use std::marker::PhantomData;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, RpcError, TimeoutCause};
use crate::types::{
    BlockReference, CryptoHash, ExecutionStatus, ExternalCall, ExternalCallResponse, Final,
    FinalExecutionOutcome, Finality, RpcTransactionResponse, TxExecutionStatus, ViewFunctionResult,
    WaitLevel,
};

use super::rpc::{RpcClient, async_sleep};

/// How long a search waits before it reads the final head again, while the
/// head is below the next height to probe.
const POLL_INTERVAL: Duration = Duration::from_millis(if cfg!(test) { 1 } else { 500 });

/// Sends an [`ExternalCall`] once and waits for the outcome of the copy
/// that answers it, looking for the copy when the send fails.
///
/// Created by [`Near::send_external_call`](crate::Near::send_external_call).
/// `W` is an executed wait level ([`ExecutedOptimistic`](crate::transaction::ExecutedOptimistic),
/// [`Executed`](crate::transaction::Executed) or [`Final`], the default); an
/// external call has no hash to wait on before its copy is included.
///
/// Awaiting it:
///
/// 1. Reads the node's final head, then sends the call at `W`. No copy
///    from this send can be included at or below that height.
/// 2. If the send times out after the node saw a copy
///    ([`TimeoutCause::Pending`] with an outcome), it waits on that copy.
/// 3. If the send fails in a way that leaves open whether the node took
///    the call (a timeout, a transport failure, a busy node:
///    [`RpcError::is_retryable`]), or the wait on the copy times out, it
///    searches every height after that final head up to
///    [`search_until`](Self::search_until), as
///    [`Near::find_external_call`](crate::Near::find_external_call) does,
///    and waits on the copy it finds, or fails with
///    [`Error::ExternalCallNotFound`]. Without `search_until` it returns the
///    error.
///
/// When the copy that answers was refused, the heights before it are
/// searched too: if the contract accepted an earlier copy, that copy is the
/// answer.
///
/// A node that rejected the call ([`Error::InvalidTx`], such as
/// [`ContractNotFound`](crate::protocol::InvalidTxError::ContractNotFound)
/// or a full chunk share) took no copy, and a node that does not track the
/// contract's shard ([`TimeoutCause::DoesNotTrackShard`]) cannot find one,
/// so those errors are returned at once.
///
/// A refused copy is an `Ok` outcome whose transaction outcome failed, like
/// a signed transaction whose action failed; one refused before its run is
/// [`Error::InvalidTx`].
///
/// # Repeats
///
/// The call is never sent twice, since a node may include a second copy:
/// it drops a repeat only once it has seen an accepted copy in a final
/// block, and keeps that record in memory. Before sending the call again
/// after an error, look for its copy with
/// [`Near::find_external_call`](crate::Near::find_external_call), up to the
/// payload's expiry. A contract that must run a payload at most once should
/// still record a nonce.
pub struct ExternalCallSend<'a, W: WaitLevel<Response = FinalExecutionOutcome> = Final> {
    rpc: Arc<RpcClient>,
    call: &'a ExternalCall,
    search_until: Option<u64>,
    _marker: PhantomData<W>,
}

impl<'a> ExternalCallSend<'a> {
    pub(crate) fn new(rpc: Arc<RpcClient>, call: &'a ExternalCall) -> Self {
        Self {
            rpc,
            call,
            search_until: None,
            _marker: PhantomData,
        }
    }
}

impl<'a, W: WaitLevel<Response = FinalExecutionOutcome>> ExternalCallSend<'a, W> {
    /// Select the executed wait level.
    pub fn wait_until<W2: WaitLevel<Response = FinalExecutionOutcome>>(
        self,
    ) -> ExternalCallSend<'a, W2> {
        ExternalCallSend {
            rpc: self.rpc,
            call: self.call,
            search_until: self.search_until,
            _marker: PhantomData,
        }
    }

    /// When the send fails, search for a copy up to `height`, usually the
    /// last height the contract accepts the payload at (its expiry). The
    /// search waits for `height` to be final, and has the same limit with
    /// fallback endpoints as
    /// [`Near::find_external_call`](crate::Near::find_external_call).
    pub fn search_until(mut self, height: u64) -> Self {
        self.search_until = Some(height);
        self
    }
}

impl<'a, W: WaitLevel<Response = FinalExecutionOutcome>> IntoFuture for ExternalCallSend<'a, W> {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = super::transport::BoxFuture<'a, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { send::<W>(&self.rpc, self.call, self.search_until).await })
    }
}

/// Dry-runs an [`ExternalCall`]: runs the contract's `__external_call` as a
/// chunk producer would before including the call, with its state init, and
/// writes nothing.
///
/// Created by [`Near::view_external_call`](crate::Near::view_external_call).
/// Awaiting it returns the bytes the contract returned and its logs; a
/// refusal is [`RpcError::ExternalCallRefused`], with the reason and the
/// logs. It runs at the final block unless [`at`](Self::at) says otherwise;
/// [`Finality::Optimistic`] is closest to what the next chunk producer
/// sees.
///
/// Only a nearcore 2.15+ node (protocol 89) runs it as an external call; an
/// older node runs `__external_call` as an ordinary view function.
pub struct ExternalCallDryRun<'a> {
    rpc: Arc<RpcClient>,
    call: &'a ExternalCall,
    block: BlockReference,
}

impl<'a> ExternalCallDryRun<'a> {
    pub(crate) fn new(rpc: Arc<RpcClient>, call: &'a ExternalCall) -> Self {
        Self {
            rpc,
            call,
            block: BlockReference::default(),
        }
    }

    /// Run at a block height, hash or finality.
    pub fn at(mut self, block: impl Into<BlockReference>) -> Self {
        self.block = block.into();
        self
    }
}

impl<'a> IntoFuture for ExternalCallDryRun<'a> {
    type Output = Result<ViewFunctionResult, Error>;
    type IntoFuture = super::transport::BoxFuture<'a, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { Ok(self.rpc.view_external_call(self.call, self.block).await?) })
    }
}

/// What to do after a send of an external call, or a wait on its copy,
/// failed.
enum Recovery {
    /// Wait on the copy with this id, then search.
    Wait(CryptoHash),
    /// Search for a copy.
    Search,
    /// Return the error.
    Stop,
}

fn recovery(error: &RpcError) -> Recovery {
    match error {
        RpcError::Timeout {
            cause:
                Some(TimeoutCause::Pending {
                    outcome: Some(outcome),
                    ..
                }),
            ..
        } => Recovery::Wait(outcome.transaction_outcome.id),
        RpcError::Timeout {
            cause: Some(TimeoutCause::DoesNotTrackShard { .. }),
            ..
        } => Recovery::Stop,
        RpcError::InvalidTx(_) => Recovery::Stop,
        error if error.is_retryable() => Recovery::Search,
        _ => Recovery::Stop,
    }
}

async fn send<W: WaitLevel<Response = FinalExecutionOutcome>>(
    rpc: &RpcClient,
    call: &ExternalCall,
    search_until: Option<u64>,
) -> Result<FinalExecutionOutcome, Error> {
    // Blocks up to the final head cannot change, so a copy of this send is
    // included above it. An optimistic head could be forked away and the
    // copy included below it.
    let first_height = final_height(rpc).await? + 1;

    let error = match rpc.send_external_call(call, W::STATUS).await {
        Ok(response) => return settle::<W>(rpc, call, first_height, executed(response)?).await,
        Err(error) => error,
    };
    let error = match recovery(&error) {
        Recovery::Stop => return Err(error.into()),
        Recovery::Search => error,
        Recovery::Wait(id) => match rpc.tx_status(&id, call.contract_id(), W::STATUS).await {
            Ok(response) => return settle::<W>(rpc, call, first_height, response).await,
            // The copy is slow to reach `W`, or a fork dropped its block.
            Err(error) if !matches!(recovery(&error), Recovery::Stop) => error,
            Err(error) => return Err(error.into()),
        },
    };

    let Some(last_height) = search_until else {
        return Err(error.into());
    };
    let heights = first_height..=last_height;
    let Some(copy) = find(rpc, call, heights.clone()).await? else {
        return Err(Error::ExternalCallNotFound {
            contract_id: call.contract_id().clone(),
            call_hash: call.call_hash(),
            heights,
        });
    };
    let response = rpc
        .tx_status(&copy.transaction_outcome.id, call.contract_id(), W::STATUS)
        .await?;
    Ok(W::convert(response, call.contract_id())?)
}

/// The answer to a send at an executed level, which carries the copy.
fn executed(response: ExternalCallResponse) -> Result<RpcTransactionResponse, Error> {
    let status = response.final_execution_status;
    let outcome = response.outcome.ok_or_else(|| {
        Error::from(RpcError::InvalidResponse(format!(
            "send_tx answered an external call at {} with no outcome",
            status.as_str()
        )))
    })?;
    let mut raw = RpcTransactionResponse::new(outcome.transaction_outcome.id, status);
    raw.outcome = Some(outcome);
    Ok(raw)
}

/// Check the copy that answers like a signed transaction's outcome, after
/// deferring a refused copy to an earlier accepted one.
async fn settle<W: WaitLevel<Response = FinalExecutionOutcome>>(
    rpc: &RpcClient,
    call: &ExternalCall,
    first_height: u64,
    response: RpcTransactionResponse,
) -> Result<FinalExecutionOutcome, Error> {
    if let Some(copy) = &response.outcome
        && refused(copy)
    {
        let height = rpc
            .block(BlockReference::from(copy.transaction_outcome.block_hash))
            .await?
            .header
            .height;
        if let Some(earlier) = find(rpc, call, first_height..=height.saturating_sub(1)).await?
            && !refused(&earlier)
        {
            let response = rpc
                .tx_status(
                    &earlier.transaction_outcome.id,
                    call.contract_id(),
                    W::STATUS,
                )
                .await?;
            return Ok(W::convert(response, call.contract_id())?);
        }
    }
    Ok(W::convert(response, call.contract_id())?)
}

async fn final_height(rpc: &RpcClient) -> Result<u64, Error> {
    Ok(rpc
        .block(BlockReference::Finality(Finality::Final))
        .await?
        .header
        .height)
}

/// Whether the copy's run refused it, or it was refused before its run.
fn refused(copy: &FinalExecutionOutcome) -> bool {
    matches!(
        copy.transaction_outcome.outcome.status,
        ExecutionStatus::Failure(_) | ExecutionStatus::InvalidTx(_)
    )
}

/// Probe `tx_status` for the copy of `call` at each of `heights`, once the
/// height is final, so a miss means there is no copy there. Returns the
/// first accepted copy, or else the earliest refused one.
pub(crate) async fn find(
    rpc: &RpcClient,
    call: &ExternalCall,
    heights: RangeInclusive<u64>,
) -> Result<Option<FinalExecutionOutcome>, Error> {
    let mut final_height = None;
    let mut first_refused = None;
    for height in heights {
        loop {
            match final_height {
                Some(final_height) if final_height >= height => break,
                Some(_) => async_sleep(POLL_INTERVAL).await,
                None => {}
            }
            final_height = Some(self::final_height(rpc).await?);
        }
        let id = call.unique_id(height);
        match rpc
            .tx_status(&id, call.contract_id(), TxExecutionStatus::None)
            .await
        {
            Ok(response) => match response.outcome {
                Some(copy) if !refused(&copy) => return Ok(Some(copy)),
                Some(copy) => {
                    first_refused.get_or_insert(copy);
                }
                None => {
                    return Err(RpcError::InvalidResponse(format!(
                        "tx_status found the external call copy {id} with no outcome"
                    ))
                    .into());
                }
            },
            Err(RpcError::TransactionNotFound(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(first_refused)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::{Value, json};

    use super::*;
    use crate::Near;
    use crate::account;
    use crate::client::test_rpc::{invalid_tx_body, rpc_error, rpc_result};
    use crate::client::{BoxFuture, RetryConfig, RpcTransport, TransportResponse};
    use crate::protocol::InvalidTxError;
    use crate::transaction::ExecutedOptimistic;
    use crate::types::SecretKey;

    /// A node that answers each method from its own queue (the last reply
    /// repeats) and records every request.
    #[derive(Default)]
    struct Node {
        replies: Mutex<HashMap<String, VecDeque<Vec<u8>>>>,
        requests: Mutex<Vec<(String, Value)>>,
    }

    struct Shared(Arc<Node>);

    impl RpcTransport for Shared {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: Value = serde_json::from_slice(&body).unwrap();
            let method = request["method"].as_str().unwrap().to_string();
            self.0
                .requests
                .lock()
                .unwrap()
                .push((method.clone(), request["params"].clone()));
            let mut replies = self.0.replies.lock().unwrap();
            let queue = replies
                .get_mut(&method)
                .unwrap_or_else(|| panic!("unexpected {method}"));
            let reply = if queue.len() > 1 {
                queue.pop_front().unwrap()
            } else {
                queue[0].clone()
            };
            if reply == TRANSPORT_FAILURE {
                return Box::pin(async { Err(RpcError::network("connection reset", None, true)) });
            }
            Box::pin(async move { Ok(TransportResponse::new(200, reply)) })
        }
    }

    impl Node {
        /// A node whose final head is 100 when a send starts.
        fn new() -> Arc<Self> {
            let node = Self::bare();
            node.reply("block", block(100));
            node
        }

        fn bare() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn reply(&self, method: &str, body: Vec<u8>) -> &Self {
            self.replies
                .lock()
                .unwrap()
                .entry(method.to_string())
                .or_default()
                .push_back(body);
            self
        }

        fn near(self: &Arc<Self>, retry_config: RetryConfig) -> Near {
            Near::custom("http://node.test", "localnet")
                .transport(Shared(self.clone()))
                .retry_config(retry_config)
                .build()
        }

        fn params(&self, method: &str) -> Vec<Value> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, _)| m == method)
                .map(|(_, params)| params.clone())
                .collect()
        }

        /// The `tx_hash` of each `EXPERIMENTAL_tx_status` with `wait_until`.
        fn lookups(&self, wait_until: &str) -> Vec<String> {
            self.params("EXPERIMENTAL_tx_status")
                .iter()
                .filter(|params| params["wait_until"] == wait_until)
                .map(|params| {
                    assert_eq!(params["sender_account_id"], "wallet.near");
                    params["tx_hash"].as_str().unwrap().to_string()
                })
                .collect()
        }
    }

    /// A reply the mock node fails at the transport instead of answering.
    const TRANSPORT_FAILURE: &[u8] = b"transport failure";

    fn no_retries() -> RetryConfig {
        RetryConfig {
            max_retries: 0,
            initial_delay_ms: 1,
            max_delay_ms: 1,
        }
    }

    fn call() -> ExternalCall {
        ExternalCall::new(account!("wallet.near"), b"pay".to_vec())
    }

    fn block_hash(height: u64) -> CryptoHash {
        CryptoHash::hash(&height.to_le_bytes())
    }

    fn block(height: u64) -> Vec<u8> {
        let hash = block_hash(height);
        let signature = SecretKey::generate_ed25519().sign(b"block").to_string();
        rpc_result(json!({
            "author": "test.near",
            "chunks": [],
            "header": {
                "height": height,
                "hash": hash,
                "prev_hash": hash,
                "prev_state_root": hash,
                "chunk_receipts_root": hash,
                "chunk_headers_root": hash,
                "chunk_tx_root": hash,
                "outcome_root": hash,
                "chunks_included": 1,
                "challenges_root": hash,
                "timestamp": 0,
                "timestamp_nanosec": "0",
                "random_value": hash,
                "gas_price": "100000000",
                "total_supply": "1",
                "last_final_block": hash,
                "last_ds_final_block": hash,
                "epoch_id": hash,
                "next_epoch_id": hash,
                "next_bp_hash": hash,
                "block_merkle_root": hash,
                "signature": signature,
                "latest_protocol_version": 89,
            },
        }))
    }

    /// The outcome of the copy of `call` at `height`, as nearcore 2.15 sends it.
    fn copy(call: &ExternalCall, height: u64, accepted: bool) -> Value {
        let id = call.unique_id(height);
        let status = if accepted {
            json!({ "SuccessValue": "Im9rIg==" })
        } else {
            json!({ "Failure": { "ActionError": { "index": 0, "kind": {
                "FunctionCallError": { "ExecutionError": "Smart contract panicked: refused" }
            } } } })
        };
        json!({
            "final_execution_status": "FINAL",
            "status": status,
            "transaction": {
                "kind": "external_call",
                "contract_id": "wallet.near",
                "payload": STANDARD.encode(call.payload()),
                "transaction_id": id,
                "state_init": null,
            },
            "transaction_outcome": {
                "id": id,
                "block_hash": block_hash(height),
                "proof": [],
                "outcome": {
                    "logs": ["checking"],
                    "receipt_ids": [],
                    "gas_burnt": 1,
                    "tokens_burnt": "0",
                    "executor_id": "wallet.near",
                    "status": status,
                    "metadata": { "version": 1, "gas_profile": null },
                },
            },
            "receipts_outcome": [],
        })
    }

    fn timeout(info: Value) -> Vec<u8> {
        rpc_error(Some("TIMEOUT_ERROR"), info, "Timeout".into())
    }

    fn not_observed() -> Vec<u8> {
        timeout(json!({ "cause": "NOT_OBSERVED" }))
    }

    fn not_found(call: &ExternalCall, height: u64) -> Vec<u8> {
        rpc_error(
            Some("UNKNOWN_TRANSACTION"),
            json!({ "requested_transaction_hash": call.unique_id(height) }),
            "doesn't exist".into(),
        )
    }

    #[tokio::test]
    async fn a_send_that_answers_returns_its_copy() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", rpc_result(copy(&call, 101, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .wait_until::<ExecutedOptimistic>()
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(101));
        assert_eq!(outcome.json::<String>().unwrap(), "ok");
        let sends = node.params("send_tx");
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0]["external_call_base64"], call.to_base64());
        assert_eq!(sends[0]["wait_until"], "EXECUTED_OPTIMISTIC");
        assert!(sends[0].get("signed_tx_base64").is_none());
    }

    #[tokio::test]
    async fn a_send_is_never_repeated_on_a_retryable_failure() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            rpc_error(Some("INTERNAL_ERROR"), json!({}), "busy".into()),
        );
        let retries = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 1,
            max_delay_ms: 1,
        };
        let near = node.near(retries);
        let error = near
            .rpc()
            .send_external_call(&call, TxExecutionStatus::Final)
            .await
            .unwrap_err();
        assert!(error.is_retryable(), "{error:?}");
        assert_eq!(node.params("send_tx").len(), 1);
        // A signed transaction's send_tx is still retried.
        let _ = near.rpc().request::<_, Value>("send_tx", json!({})).await;
        assert_eq!(node.params("send_tx").len(), 5);
    }

    #[tokio::test]
    async fn a_pending_timeout_waits_on_the_copy_it_names() {
        let call = call();
        let node = Node::new();
        let mut pending = copy(&call, 101, true);
        pending["final_execution_status"] = "EXECUTED_OPTIMISTIC".into();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "PENDING", "status": pending })),
        );
        node.reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 101, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(101));
        assert_eq!(node.params("send_tx").len(), 1, "no resend");
        assert_eq!(node.lookups("FINAL"), [call.unique_id(101).to_string()]);
    }

    #[tokio::test]
    async fn the_pending_copy_id_is_the_timeout_hash() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "PENDING", "status": copy(&call, 101, true) })),
        );
        let error = node
            .near(no_retries())
            .rpc()
            .send_external_call(&call, TxExecutionStatus::Final)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RpcError::Timeout { transaction_hash: Some(id), .. } if id == call.unique_id(101)
        ));
        node.reply("send_tx", not_observed());
        node.replies
            .lock()
            .unwrap()
            .get_mut("send_tx")
            .unwrap()
            .pop_front();
        let error = node
            .near(no_retries())
            .rpc()
            .send_external_call(&call, TxExecutionStatus::Final)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RpcError::Timeout {
                transaction_hash: None,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn a_node_without_the_shard_gets_no_resend() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "DOES_NOT_TRACK_SHARD", "shard_id": 3 })),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(200)
            .await
            .unwrap_err();
        assert!(matches!(
            error.as_rpc(),
            Some(RpcError::Timeout {
                cause: Some(TimeoutCause::DoesNotTrackShard { shard_id: 3 }),
                ..
            })
        ));
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn a_missing_contract_fails_at_once() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            invalid_tx_body(json!({ "ContractNotFound": { "contract_id": "wallet.near" } })),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(200)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                Error::InvalidTx(ref e) if matches!(**e, InvalidTxError::ContractNotFound { .. })
            ),
            "{error:?}"
        );
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn an_unobserved_send_without_a_search_returns_the_timeout() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", not_observed());
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap_err();
        assert!(matches!(error.as_rpc(), Some(RpcError::Timeout { .. })));
        assert_eq!(node.params("send_tx").len(), 1);
        assert_eq!(
            node.params("block").len(),
            1,
            "only the final head, no search"
        );
    }

    #[tokio::test]
    async fn an_unobserved_send_searches_from_the_final_head_and_waits_on_the_copy() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", not_observed())
            .reply("block", block(105))
            .reply("EXPERIMENTAL_tx_status", not_found(&call, 101))
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 102, true)))
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 102, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(110)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(102));
        assert_eq!(
            node.lookups("NONE"),
            [
                call.unique_id(101).to_string(),
                call.unique_id(102).to_string()
            ]
        );
        assert_eq!(node.lookups("FINAL"), [call.unique_id(102).to_string()]);
    }

    #[tokio::test]
    async fn a_search_that_finds_no_copy_names_the_heights() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", not_observed())
            .reply("block", block(105))
            .reply("EXPERIMENTAL_tx_status", not_found(&call, 101));
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(103)
            .await
            .unwrap_err();
        match error {
            Error::ExternalCallNotFound {
                contract_id,
                call_hash,
                heights,
            } => {
                assert_eq!(contract_id, "wallet.near");
                assert_eq!(call_hash, call.call_hash());
                assert_eq!(heights, 101..=103);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(node.lookups("NONE").len(), 3);
    }

    #[tokio::test]
    async fn a_refused_answer_with_no_earlier_copy_is_the_answer() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", rpc_result(copy(&call, 103, false)))
            .reply("block", block(103))
            .reply("EXPERIMENTAL_tx_status", not_found(&call, 101));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(103));
        assert!(outcome.is_failure());
        assert_eq!(node.lookups("NONE").len(), 2, "heights 101 and 102");
    }

    #[tokio::test]
    async fn find_prefers_an_accepted_copy_and_falls_back_to_the_first_refused() {
        let call = call();
        let node = Node::new();
        node.reply("block", block(110))
            .reply(
                "EXPERIMENTAL_tx_status",
                rpc_result(copy(&call, 101, false)),
            )
            .reply(
                "EXPERIMENTAL_tx_status",
                rpc_result(copy(&call, 102, false)),
            )
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 103, true)));
        let near = node.near(no_retries());
        let found = near
            .find_external_call(&call, 101..=105)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.transaction_hash(), &call.unique_id(103));
        assert_eq!(node.lookups("NONE").len(), 3, "stops at the accepted copy");

        let node = Node::new();
        node.reply("block", block(110))
            .reply(
                "EXPERIMENTAL_tx_status",
                rpc_result(copy(&call, 101, false)),
            )
            .reply(
                "EXPERIMENTAL_tx_status",
                rpc_result(copy(&call, 102, false)),
            )
            .reply("EXPERIMENTAL_tx_status", not_found(&call, 103));
        let near = node.near(no_retries());
        let found = near
            .find_external_call(&call, 101..=104)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.transaction_hash(), &call.unique_id(101));
        assert_eq!(node.lookups("NONE").len(), 4);
    }

    #[tokio::test]
    async fn find_probes_a_height_only_once_it_is_final() {
        let call = call();
        let node = Node::bare();
        node.reply("block", block(100))
            .reply("block", block(101))
            .reply("block", block(103))
            .reply("EXPERIMENTAL_tx_status", not_found(&call, 101));
        let near = node.near(no_retries());
        assert!(
            near.find_external_call(&call, 101..=103)
                .await
                .unwrap()
                .is_none()
        );
        let order: Vec<String> = node
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(method, params)| match method.as_str() {
                "block" => "final".to_string(),
                _ => params["tx_hash"].as_str().unwrap().to_string(),
            })
            .collect();
        let id = |h: u64| call.unique_id(h).to_string();
        assert_eq!(
            order,
            ["final", "final", &id(101), "final", &id(102), &id(103)]
        );
        assert_eq!(node.params("block")[0]["finality"], "final");
    }

    #[tokio::test]
    async fn find_stops_on_a_node_without_the_shard() {
        let call = call();
        let node = Node::new();
        node.reply("block", block(110)).reply(
            "EXPERIMENTAL_tx_status",
            rpc_error(Some("DOES_NOT_TRACK_SHARD"), json!({}), "no shard".into()),
        );
        let error = node
            .near(no_retries())
            .find_external_call(&call, 101..=105)
            .await
            .unwrap_err();
        assert!(matches!(error.as_rpc(), Some(RpcError::ShardNotTracked)));
        assert_eq!(node.lookups("NONE").len(), 1);
    }

    #[tokio::test]
    async fn find_of_a_copy_with_no_outcome_is_an_invalid_response() {
        let call = call();
        let node = Node::new();
        node.reply("block", block(110)).reply(
            "EXPERIMENTAL_tx_status",
            rpc_result(json!({ "final_execution_status": "NONE" })),
        );
        let error = node
            .near(no_retries())
            .find_external_call(&call, 101..=101)
            .await
            .unwrap_err();
        assert!(
            matches!(error.as_rpc(), Some(RpcError::InvalidResponse(m)) if m.contains("no outcome")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn find_of_an_empty_range_asks_nothing() {
        let call = call();
        let node = Node::new();
        #[allow(clippy::reversed_empty_ranges)]
        let found = node
            .near(no_retries())
            .find_external_call(&call, 101..=100)
            .await
            .unwrap();
        assert!(found.is_none());
        assert!(node.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_transport_failure_is_not_resent() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", TRANSPORT_FAILURE.to_vec())
            .reply("send_tx", rpc_result(copy(&call, 101, true)));
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap_err();
        assert!(
            matches!(error.as_rpc(), Some(RpcError::Network { .. })),
            "{error:?}"
        );
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn a_rejected_call_is_returned_without_a_search() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            invalid_tx_body(json!({
                "ExternalCallChunkShareReached": { "share": { "Compute": { "used": 2, "limit": 1 } } }
            })),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(200)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                Error::InvalidTx(ref e)
                    if matches!(**e, InvalidTxError::ExternalCallChunkShareReached { .. })
            ),
            "{error:?}"
        );
        assert_eq!(node.params("send_tx").len(), 1);
        assert!(node.params("EXPERIMENTAL_tx_status").is_empty());
    }

    #[tokio::test]
    async fn a_transport_failure_searches_for_the_copy() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", TRANSPORT_FAILURE.to_vec())
            .reply("block", block(105))
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 101, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(105)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(101));
        assert_eq!(node.params("send_tx").len(), 1);
        assert_eq!(node.lookups("NONE"), [call.unique_id(101).to_string()]);
    }

    #[tokio::test]
    async fn the_search_starts_just_above_the_final_head() {
        let call = call();
        let node = Node::bare();
        node.reply("block", block(90))
            .reply("send_tx", TRANSPORT_FAILURE.to_vec())
            .reply("block", block(95))
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 91, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(95)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(91));
        assert_eq!(node.params("block")[0]["finality"], "final");
        assert_eq!(node.lookups("NONE"), [call.unique_id(91).to_string()]);
    }

    #[tokio::test]
    async fn a_call_over_the_size_limit_is_sent_once() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            invalid_tx_body(json!({
                "TransactionSizeExceeded": { "size": 20481, "limit": 20480 }
            })),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(200)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                Error::InvalidTx(ref e)
                    if matches!(**e, InvalidTxError::TransactionSizeExceeded { size: 20481, limit: 20480, .. })
            ),
            "{error:?}"
        );
        assert_eq!(node.params("send_tx").len(), 1);
        assert_eq!(
            node.params("block").len(),
            1,
            "only the final head, no search"
        );
    }

    #[tokio::test]
    async fn a_copy_that_never_reaches_the_level_is_searched_for_not_resent() {
        let call = call();
        let node = Node::new();
        let mut pending = copy(&call, 101, true);
        pending["final_execution_status"] = "INCLUDED".into();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "PENDING", "status": pending })),
        );
        node.reply("EXPERIMENTAL_tx_status", not_observed());
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap_err();
        assert!(
            matches!(error.as_rpc(), Some(RpcError::Timeout { .. })),
            "{error:?}"
        );
        assert_eq!(
            node.params("send_tx").len(),
            1,
            "a known copy is not resent"
        );

        // With a search, the copy is found at its height once final.
        let node = Node::new();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "PENDING", "status": pending })),
        )
        .reply("block", block(105))
        .reply("EXPERIMENTAL_tx_status", not_observed())
        .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 101, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(105)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(101));
        assert_eq!(node.params("send_tx").len(), 1);
        assert_eq!(node.lookups("NONE"), [call.unique_id(101).to_string()]);
    }

    #[tokio::test]
    async fn a_wait_on_a_copy_without_the_shard_is_not_resent() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            timeout(json!({ "cause": "PENDING", "status": copy(&call, 101, true) })),
        )
        .reply(
            "EXPERIMENTAL_tx_status",
            rpc_error(Some("DOES_NOT_TRACK_SHARD"), json!({}), "no shard".into()),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .search_until(200)
            .await
            .unwrap_err();
        assert!(matches!(error.as_rpc(), Some(RpcError::ShardNotTracked)));
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn a_refused_first_answer_defers_to_an_earlier_accepted_copy() {
        let call = call();
        let node = Node::new();
        node.reply("send_tx", rpc_result(copy(&call, 103, false)))
            .reply("block", block(103))
            .reply("EXPERIMENTAL_tx_status", rpc_result(copy(&call, 101, true)));
        let outcome = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap();
        assert_eq!(outcome.transaction_hash(), &call.unique_id(101));
        assert!(outcome.is_success());
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn an_executed_answer_without_an_outcome_is_an_invalid_response() {
        let call = call();
        let node = Node::new();
        node.reply(
            "send_tx",
            rpc_result(json!({ "final_execution_status": "FINAL" })),
        );
        let error = node
            .near(no_retries())
            .send_external_call(&call)
            .await
            .unwrap_err();
        assert!(
            matches!(error.as_rpc(), Some(RpcError::InvalidResponse(m)) if m.contains("FINAL")),
            "{error:?}"
        );
        assert_eq!(node.params("send_tx").len(), 1);
    }

    #[tokio::test]
    async fn a_dry_run_reads_the_final_block_unless_told_otherwise() {
        let call = call();
        let node = Node::new();
        node.reply(
            "EXPERIMENTAL_call_function",
            rpc_result(json!({
                "result": b"\"ok\"".to_vec(),
                "logs": ["checking", "accepted"],
                "block_height": 9,
                "block_hash": block_hash(9),
            })),
        );
        let near = node.near(no_retries());
        let result = near.view_external_call(&call).await.unwrap();
        assert_eq!(result.json::<String>().unwrap(), "ok");
        assert_eq!(result.logs, ["checking", "accepted"]);
        near.view_external_call(&call)
            .at(Finality::Optimistic)
            .await
            .unwrap();
        let params = node.params("EXPERIMENTAL_call_function");
        assert_eq!(params[0]["finality"], "final");
        assert_eq!(params[1]["finality"], "optimistic");
        for params in &params {
            assert_eq!(params["account_id"], "wallet.near");
            assert_eq!(params["method_name"], "__external_call");
            assert_eq!(params["args_base64"], STANDARD.encode(call.payload()));
        }
    }

    /// How the simulated node answers one send.
    #[derive(Clone, Copy, Debug)]
    enum SendReply {
        /// As nearcore's `answer_to_send` does for a send that arrived when
        /// the head was at `sent_at`, with every copy executed and final.
        Answer { sent_at: u64 },
        /// Times out naming the copy `Answer` would give, before it is final.
        Pending { sent_at: u64 },
        /// Times out naming a copy whose block a fork dropped.
        Orphaned,
        /// Times out before seeing any copy.
        NotObserved,
        /// Fails at the transport.
        Transport,
    }

    /// A chain with at most one copy of the call per height. Its final head
    /// moves up one block each time it is read, to 104.
    struct Chain {
        call: ExternalCall,
        /// Height to whether the contract accepted the copy there.
        copies: std::collections::BTreeMap<u64, bool>,
        /// The final head the last read returned.
        final_head: Mutex<u64>,
        sends: Mutex<VecDeque<SendReply>>,
        requests: Mutex<Vec<(String, Value)>>,
        /// Probes of a height the client had not seen final.
        early_probes: Mutex<Vec<u64>>,
    }

    impl Chain {
        /// nearcore 2.15's `ExternalCallCache::answer_to_send`: the latest
        /// copy after `sent_at`; else the first accepted copy (all are
        /// final and inside the repeat window).
        fn answer(&self, sent_at: u64) -> Option<u64> {
            let (&latest, _) = self.copies.last_key_value()?;
            if latest > sent_at {
                return Some(latest);
            }
            self.copies
                .iter()
                .find(|(_, accepted)| **accepted)
                .map(|(height, _)| *height)
        }

        fn height_of(&self, id: &str) -> Option<u64> {
            self.copies
                .keys()
                .copied()
                .find(|h| self.call.unique_id(*h).to_string() == id)
        }

        fn copy_at(&self, height: u64) -> Value {
            copy(&self.call, height, self.copies[&height])
        }

        fn reply(&self, method: &str, params: &Value) -> Vec<u8> {
            match method {
                // The optimistic head runs three blocks ahead of the final one.
                "block" if params.get("finality").is_some_and(|f| f != "final") => {
                    block(*self.final_head.lock().unwrap() + 3)
                }
                "block" if params.get("finality").is_some() => {
                    let mut final_head = self.final_head.lock().unwrap();
                    *final_head = (*final_head + 1).min(104);
                    block(*final_head)
                }
                "block" => {
                    let hash = params["block_id"].as_str().unwrap();
                    let height = self
                        .copies
                        .keys()
                        .copied()
                        .find(|h| block_hash(*h).to_string() == hash)
                        .unwrap_or_else(|| panic!("block {hash} has no copy"));
                    block(height)
                }
                "EXPERIMENTAL_tx_status" => {
                    let id = params["tx_hash"].as_str().unwrap();
                    let probed = (101..=104).find(|h| self.call.unique_id(*h).to_string() == id);
                    if params["wait_until"] == "NONE"
                        && let Some(height) = probed
                        && height > *self.final_head.lock().unwrap()
                    {
                        self.early_probes.lock().unwrap().push(height);
                    }
                    match self.height_of(id) {
                        Some(height) => rpc_result(self.copy_at(height)),
                        None if params["wait_until"] == "NONE" => rpc_error(
                            Some("UNKNOWN_TRANSACTION"),
                            json!({ "requested_transaction_hash": id }),
                            "doesn't exist".into(),
                        ),
                        None => not_observed(),
                    }
                }
                "send_tx" => {
                    let reply = self
                        .sends
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("a second send");
                    match reply {
                        SendReply::Answer { sent_at } => match self.answer(sent_at) {
                            Some(height) => rpc_result(self.copy_at(height)),
                            None => not_observed(),
                        },
                        SendReply::Pending { sent_at } => match self.answer(sent_at) {
                            Some(height) => {
                                let mut pending = self.copy_at(height);
                                pending["final_execution_status"] = "INCLUDED".into();
                                timeout(json!({ "cause": "PENDING", "status": pending }))
                            }
                            None => not_observed(),
                        },
                        SendReply::Orphaned => {
                            let mut pending = copy(&self.call, 999, true);
                            pending["final_execution_status"] = "INCLUDED".into();
                            timeout(json!({ "cause": "PENDING", "status": pending }))
                        }
                        SendReply::NotObserved => not_observed(),
                        SendReply::Transport => TRANSPORT_FAILURE.to_vec(),
                    }
                }
                other => panic!("unexpected {other}"),
            }
        }
    }

    struct ChainTransport(Arc<Chain>);

    impl RpcTransport for ChainTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: Value = serde_json::from_slice(&body).unwrap();
            let method = request["method"].as_str().unwrap().to_string();
            let params = request["params"].clone();
            let reply = self.0.reply(&method, &params);
            self.0.requests.lock().unwrap().push((method, params));
            if reply == TRANSPORT_FAILURE {
                return Box::pin(async { Err(RpcError::network("connection reset", None, true)) });
            }
            Box::pin(async move { Ok(TransportResponse::new(200, reply)) })
        }
    }

    /// Every chain of copies at heights 101 to 104, every send reply and
    /// both search settings, checked against what must hold whatever the
    /// node does:
    ///
    /// - the call is sent once;
    /// - a probe asks only for a height in the searched range, once the
    ///   client has read a final head at or above it;
    /// - the answer is a copy on the chain;
    /// - an answer is never a refused copy while the chain has an accepted
    ///   one, and a search never misses a copy.
    #[tokio::test]
    async fn the_send_settles_on_the_right_copy_whatever_the_node_does() {
        let call = call();
        let first_sends = [
            SendReply::Answer { sent_at: 100 },
            SendReply::Pending { sent_at: 100 },
            SendReply::Orphaned,
            SendReply::NotObserved,
            SendReply::Transport,
        ];
        let mut runs = 0;
        for chain_index in 0..81u32 {
            let copies: std::collections::BTreeMap<u64, bool> = (0..4)
                .filter_map(|i| match chain_index / 3u32.pow(i) % 3 {
                    0 => None,
                    kind => Some((101 + u64::from(i), kind == 1)),
                })
                .collect();
            let any_copy = !copies.is_empty();
            let any_accepted = copies.values().any(|accepted| *accepted);
            for first in first_sends {
                for search_until in [None, Some(104)] {
                    runs += 1;
                    let chain = Arc::new(Chain {
                        call: call.clone(),
                        copies: copies.clone(),
                        // The send's first read gives 100.
                        final_head: Mutex::new(99),
                        sends: Mutex::new(VecDeque::from([first])),
                        requests: Mutex::default(),
                        early_probes: Mutex::default(),
                    });
                    let near = Near::custom("http://node.test", "localnet")
                        .transport(ChainTransport(chain.clone()))
                        .retry_config(no_retries())
                        .build();
                    let mut send = near.send_external_call(&call);
                    if let Some(height) = search_until {
                        send = send.search_until(height);
                    }
                    let result = send.await;
                    let case = format!(
                        "copies {copies:?}, send {first:?}, \
                         search until {search_until:?}: {result:?}"
                    );

                    let requests = chain.requests.lock().unwrap();
                    let sends = requests.iter().filter(|(m, _)| m == "send_tx").count();
                    assert_eq!(sends, 1, "{case}");
                    assert_eq!(
                        *chain.early_probes.lock().unwrap(),
                        Vec::<u64>::new(),
                        "probed before final: {case}"
                    );
                    for (_, params) in requests
                        .iter()
                        .filter(|(m, p)| m == "EXPERIMENTAL_tx_status" && p["wait_until"] == "NONE")
                    {
                        let id = params["tx_hash"].as_str().unwrap();
                        (101..=104)
                            .find(|h| call.unique_id(*h).to_string() == id)
                            .unwrap_or_else(|| panic!("probe of {id} outside the range: {case}"));
                    }

                    match &result {
                        Ok(outcome) => {
                            let id = outcome.transaction_hash().to_string();
                            let height = chain
                                .height_of(&id)
                                .unwrap_or_else(|| panic!("answer is no copy: {case}"));
                            assert_eq!(outcome.is_success(), copies[&height], "{case}");
                            if any_accepted {
                                assert!(outcome.is_success(), "refused over accepted: {case}");
                            }
                        }
                        Err(Error::ExternalCallNotFound { heights, .. }) => {
                            assert!(search_until.is_some() && !any_copy, "{case}");
                            assert_eq!(*heights, 101..=104, "{case}");
                        }
                        Err(_) => assert!(search_until.is_none() || !any_copy, "{case}"),
                    }
                    if search_until.is_some() && any_copy {
                        assert!(result.is_ok(), "a search misses a copy: {case}");
                    }
                }
            }
        }
        assert_eq!(runs, 81 * 5 * 2);
    }
}
