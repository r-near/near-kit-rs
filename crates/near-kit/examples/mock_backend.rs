//! Testing contract code against a mock backend, with no network.
//!
//! Generated `#[contract]` clients and the token clients don't name an RPC
//! client: their views need a `Viewer` and their calls a `TxSender`. `Near`
//! and `SigningNear` implement both, and so can a test double. Write app code
//! generic over the backend, pass a `SigningNear` in production and a mock in
//! tests.
//!
//! What it shows:
//!
//! - `MockChain`: a `Viewer` that answers views from a table, and a `TxSender`
//!   that records each transaction (receiver and actions) instead of sending.
//! - A `#[contract]` client and a `FungibleTokenClient` built on it with
//!   `Contract::client(backend, contract_id)`.
//! - App code (`tip`) generic over the backend, which checks each call's
//!   output (a send returns `Ok` even when an action failed), and assertions
//!   on exactly what it would have sent: method, JSON arguments, deposit and
//!   gas. The mock can fail a method, to cover the failure path.
//!
//! In a real crate the checks in `main` would be `#[tokio::test]`s. For
//! canned JSON-RPC responses one layer lower, implement `RpcTransport` and
//! pass it to `NearBuilder::transport`.
//!
//! Run it:
//!
//! ```text
//! cargo run -p near-kit --example mock_backend --features contracts
//! ```

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use near_kit::protocol::{Action, FunctionCallAction};
use near_kit::rpc::{BlockReference, FinalExecutionOutcome, ViewFunctionResult};
use near_kit::standards::{FungibleToken, FungibleTokenClient};
use near_kit::{
    AccountId, Contract, ContractError, CryptoHash, Gas, NearToken, TxSender, Viewer, account,
};
use serde::Serialize;

// --- The contract and the app code under test ---

#[near_kit::contract]
pub trait Guestbook {
    fn total_messages(&self) -> u32;

    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
}

#[derive(Serialize)]
pub struct AddMessageArgs {
    pub text: String,
}

/// What app code reads from a sent call's output: whether its actions
/// succeeded.
///
/// A send's `Ok` only means the transaction ran: with `SigningNear`, a
/// transfer that fails for lack of tokens is `Ok(outcome)` with a failed
/// action. Each backend's output says so in its own way.
trait CheckOutput {
    fn check(&self) -> Result<(), Box<dyn Error>>;
}

/// `SigningNear`'s output.
impl CheckOutput for FinalExecutionOutcome {
    fn check(&self) -> Result<(), Box<dyn Error>> {
        self.result()?;
        Ok(())
    }
}

/// Send `whole_tokens` of `token` to `receiver`, registering the receiver
/// first if it can't hold the token yet.
///
/// Generic over the backend: in production `C` is a `SigningNear`. The
/// `'static` bounds are there because awaiting a `View` or `Call` boxes a
/// `'static` future that owns a clone of the backend.
async fn tip<C>(
    token: &FungibleTokenClient<C>,
    receiver: AccountId,
    whole_tokens: u128,
) -> Result<(), Box<dyn Error>>
where
    C: Viewer + TxSender + Clone + 'static,
    C::Output: CheckOutput,
    C::ViewError: fmt::Debug + fmt::Display + 'static,
    C::SendError: fmt::Debug + fmt::Display + 'static,
{
    let decimals = token.metadata().await?.decimals;
    if !token.is_registered(receiver.clone()).await? {
        let bounds = token.storage_balance_bounds().await?;
        token
            .storage_deposit(receiver.clone(), bounds.min)
            .await?
            .check()?;
    }
    let amount = whole_tokens * 10u128.pow(decimals.into());
    token
        .transfer(receiver, amount, Some("tip".into()))
        .await?
        .check()?;
    Ok(())
}

// --- The mock ---

/// Canned view results, by contract and method.
type ViewTable = HashMap<(AccountId, String), serde_json::Value>;

/// Transactions "sent" so far: receiver and actions.
type SentLog = Vec<(AccountId, Vec<Action>)>;

/// What the mock's sends return: the failure, if the transaction called a
/// method the mock was told to fail.
#[derive(Debug)]
struct MockOutcome {
    failure: Option<String>,
}

impl CheckOutput for MockOutcome {
    fn check(&self) -> Result<(), Box<dyn Error>> {
        match &self.failure {
            Some(failure) => Err(failure.clone().into()),
            None => Ok(()),
        }
    }
}

/// A fake chain: views come from a table, transactions are recorded.
///
/// Clones share the table and the record, so a test keeps one clone to
/// inspect while the clients hold others.
#[derive(Clone)]
struct MockChain {
    account_id: AccountId,
    views: Arc<Mutex<ViewTable>>,
    sent: Arc<Mutex<SentLog>>,
    failing: Arc<Mutex<HashSet<String>>>,
}

impl MockChain {
    fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            views: Arc::default(),
            sent: Arc::default(),
            failing: Arc::default(),
        }
    }

    /// Make every call of `method` fail on chain: the send still succeeds,
    /// as a real one does, and its output reports the failed action.
    fn fail(&self, method: &str) {
        self.failing.lock().unwrap().insert(method.to_owned());
    }

    /// Answer every call of `method` on `contract` with `value`.
    fn answer(&self, contract: AccountId, method: &str, value: serde_json::Value) {
        self.views
            .lock()
            .unwrap()
            .insert((contract, method.to_owned()), value);
    }

    /// The function calls sent so far, as (receiver, call).
    fn calls(&self) -> Vec<(AccountId, FunctionCallAction)> {
        let sent = self.sent.lock().unwrap();
        let mut calls = Vec::new();
        for (receiver, actions) in sent.iter() {
            for action in actions {
                if let Action::FunctionCall(call) = action {
                    calls.push((receiver.clone(), call.clone()));
                }
            }
        }
        calls
    }
}

impl Viewer for MockChain {
    type ViewError = String;

    async fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        let views = self.views.lock().unwrap();
        let value = views
            .get(&(contract_id.clone(), method.to_owned()))
            .ok_or_else(|| format!("no answer for {contract_id}.{method}"))?;
        let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        // Any block height and hash will do for a mock.
        Ok(ViewFunctionResult::new(bytes, 1, CryptoHash::ZERO))
    }
}

impl TxSender for MockChain {
    // A real backend returns an outcome; the mock reports only a failure.
    type Output = MockOutcome;
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    async fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<MockOutcome, String> {
        let failing = self.failing.lock().unwrap();
        let failure = actions.iter().find_map(|action| match action {
            Action::FunctionCall(call) if failing.contains(&call.method_name) => {
                Some(format!("{receiver_id}.{} failed", call.method_name))
            }
            _ => None,
        });
        self.sent.lock().unwrap().push((receiver_id, actions));
        Ok(MockOutcome { failure })
    }
}

/// A chain where `token` is a 6-decimal token, and bob is registered on it
/// or not.
fn usdc_chain(token: &AccountId, bob_registered: bool) -> MockChain {
    let chain = MockChain::new(account!("alice.near"));
    chain.answer(
        token.clone(),
        "ft_metadata",
        serde_json::json!({ "spec": "ft-1.0.0", "name": "USD Coin", "symbol": "USDC", "decimals": 6 }),
    );
    chain.answer(
        token.clone(),
        "storage_balance_bounds",
        serde_json::json!({ "min": "1250000000000000000000", "max": null }),
    );
    // NEP-145: `null` for an account that isn't registered.
    let storage = if bob_registered {
        serde_json::json!({ "total": "1250000000000000000000", "available": "0" })
    } else {
        serde_json::Value::Null
    };
    chain.answer(token.clone(), "storage_balance_of", storage);
    chain
}

/// A function call's arguments as JSON.
fn json_args(call: &FunctionCallAction) -> serde_json::Value {
    serde_json::from_slice(&call.args).expect("JSON arguments")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // --- A #[contract] client on the mock ---
    let chain = MockChain::new(account!("alice.near"));
    chain.answer(
        account!("guestbook.near"),
        "total_messages",
        serde_json::json!(7),
    );

    let guestbook = Guestbook::client(chain.clone(), account!("guestbook.near"));
    assert_eq!(guestbook.total_messages().await?, 7);

    guestbook
        .add_message(AddMessageArgs { text: "hi".into() })
        .gas(Gas::from_tgas(10))
        .await?
        .check()?;

    let calls = chain.calls();
    assert_eq!(calls.len(), 1);
    let (receiver, call) = &calls[0];
    assert_eq!(receiver, "guestbook.near");
    assert_eq!(call.method_name, "add_message");
    assert_eq!(json_args(call), serde_json::json!({ "text": "hi" }));
    assert_eq!(call.gas, Gas::from_tgas(10));
    assert_eq!(call.deposit, NearToken::ZERO);
    println!("guestbook: 1 view answered, 1 call recorded");

    // A view the mock can't answer is the backend's error.
    let token_without_answers = FungibleToken::client(chain.clone(), account!("unknown.near"));
    match token_without_answers.total_supply().await {
        Err(ContractError::Backend(error)) => println!("unanswered view: {error}"),
        other => return Err(format!("expected a backend error, got {other:?}").into()),
    }

    // --- App code on a FungibleTokenClient over the mock ---
    let usdc = account!("usdc.near");
    let chain = usdc_chain(&usdc, false);
    let token = FungibleToken::client(chain.clone(), usdc.clone());
    tip(&token, account!("bob.near"), 5).await?;

    let calls = chain.calls();
    let methods: Vec<&str> = calls
        .iter()
        .map(|(_, call)| call.method_name.as_str())
        .collect();
    assert_eq!(methods, ["storage_deposit", "ft_transfer"]);
    assert!(calls.iter().all(|(receiver, _)| receiver == &usdc));

    let (_, register) = &calls[0];
    assert_eq!(
        json_args(register),
        serde_json::json!({ "account_id": "bob.near", "registration_only": true })
    );
    assert_eq!(
        register.deposit,
        NearToken::from_yoctonear(1_250_000_000_000_000_000_000)
    );

    let (_, transfer) = &calls[1];
    assert_eq!(
        json_args(transfer),
        serde_json::json!({ "receiver_id": "bob.near", "amount": "5000000", "memo": "tip" })
    );
    // NEP-141 requires exactly 1 yoctoNEAR on ft_transfer.
    assert_eq!(transfer.deposit, NearToken::from_yoctonear(1));
    println!("tip to an unregistered account: {methods:?}");

    // Once bob is registered, the same code only transfers.
    let chain = usdc_chain(&usdc, true);
    let token = FungibleToken::client(chain.clone(), usdc.clone());
    tip(&token, account!("bob.near"), 5).await?;
    let methods: Vec<String> = chain
        .calls()
        .into_iter()
        .map(|(_, call)| call.method_name)
        .collect();
    assert_eq!(methods, ["ft_transfer"]);
    println!("tip to a registered account: {methods:?}");

    // A transfer that fails on chain (too few tokens, say) is an error from
    // `tip`, not a silent success.
    let chain = usdc_chain(&usdc, true);
    chain.fail("ft_transfer");
    let token = FungibleToken::client(chain.clone(), usdc.clone());
    match tip(&token, account!("bob.near"), 5).await {
        Err(error) => println!("failed transfer: {error}"),
        Ok(()) => return Err("a failed transfer must not look successful".into()),
    }

    println!("done");
    Ok(())
}
