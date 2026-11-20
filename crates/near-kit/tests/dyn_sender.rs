//! A sender picked at run time, behind one type: an object-safe trait in
//! front of `TxSender`, built from near-kit's public API alone.
//!
//! `Arc<dyn DynTxSender>` is then a `TxSender` like any other: generic code,
//! a `#[contract]` client's calls and `.transaction(..)` all accept it. Its
//! output is a `SendTxResponse` and its error a boxed `std::error::Error`,
//! which downcasts back to the concrete sender's error.
#![cfg(all(feature = "contracts", feature = "rpc"))]

use std::fmt;
use std::future::{Future, IntoFuture};
use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::rpc::{BoxFuture, SendTxResponse};
use near_kit::transaction::{CodecError, FunctionCall};
use near_kit::*;
use serde_json::json;

// ---- adapter: begin ----

/// The error of a type-erased sender.
pub type DynSendError = Box<dyn std::error::Error + Send + Sync>;

/// An object-safe [`TxSender`]: every `TxSender` whose error is a
/// `std::error::Error` implements it.
pub trait DynTxSender: MaybeSend + MaybeSync {
    fn dyn_account_id(&self) -> &AccountId;

    fn dyn_chain_id(&self) -> &str;

    fn dyn_send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynSendError>>;
}

impl<S> DynTxSender for S
where
    S: TxSender,
    S::SendError: Into<DynSendError>,
{
    fn dyn_account_id(&self) -> &AccountId {
        self.account_id()
    }

    fn dyn_chain_id(&self) -> &str {
        self.chain_id()
    }

    fn dyn_send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynSendError>> {
        Box::pin(async move {
            match self.send(receiver_id, actions).await {
                Ok(output) => Ok(output.into()),
                Err(error) => Err(error.into()),
            }
        })
    }
}

impl TxSender for dyn DynTxSender + '_ {
    type Output = SendTxResponse;
    type SendError = DynSendError;

    fn account_id(&self) -> &AccountId {
        self.dyn_account_id()
    }

    fn chain_id(&self) -> &str {
        self.dyn_chain_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<SendTxResponse, DynSendError>> + MaybeSend {
        self.dyn_send(receiver_id, actions)
    }
}

/// A shared, type-erased sender.
pub type ArcTxSender = Arc<dyn DynTxSender>;

// ---- adapter: end ----

#[near_kit::contract]
pub trait Counter {
    #[call]
    fn increment(&mut self, by: u64);
}

/// The recorder's own error. It isn't `CodecError`, so the adapter erases
/// a real error type.
#[derive(Debug)]
enum RecorderError {
    Refused,
    Codec(CodecError),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => f.write_str("the recorder refuses deposits"),
            Self::Codec(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RecorderError {}

impl From<CodecError> for RecorderError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

type Sent = (AccountId, Vec<Action>);

/// Records every transaction and refuses any transfer. Its chain ID is a
/// plain `String`.
struct Recorder {
    account_id: AccountId,
    chain_id: String,
    sent: Arc<Mutex<Vec<Sent>>>,
}

impl Recorder {
    fn new() -> (Self, Arc<Mutex<Vec<Sent>>>) {
        let sent = Arc::default();
        let recorder = Self {
            account_id: account!("alice.testnet"),
            chain_id: "testnet".to_owned(),
            sent: Arc::clone(&sent),
        };
        (recorder, sent)
    }
}

impl TxSender for Recorder {
    type Output = SendTxResponse;
    type SendError = RecorderError;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    async fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<SendTxResponse, RecorderError> {
        if actions.iter().any(|a| matches!(a, Action::Transfer(_))) {
            return Err(RecorderError::Refused);
        }
        self.sent.lock().unwrap().push((receiver_id, actions));
        Ok(SendTxResponse::new(
            CryptoHash::ZERO,
            self.account_id.clone(),
            None,
        ))
    }
}

fn counter() -> AccountId {
    account!("counter.testnet")
}

fn erased() -> (ArcTxSender, Arc<Mutex<Vec<Sent>>>) {
    let (recorder, sent) = Recorder::new();
    (Arc::new(recorder), sent)
}

fn method_names(sent: &[Sent]) -> Vec<String> {
    sent.iter()
        .flat_map(|(_, actions)| actions)
        .map(|action| match action {
            Action::FunctionCall(call) => call.method_name.clone(),
            other => panic!("expected a function call, got {other:?}"),
        })
        .collect()
}

/// Code written against `TxSender` alone.
async fn bump<S: TxSender>(sender: &S) -> Result<SendTxResponse, S::SendError> {
    let output = sender
        .send(counter(), vec![FunctionCall::new("bump").try_into()?])
        .await?;
    Ok(output.into())
}

#[tokio::test]
async fn an_erased_sender_is_a_tx_sender_in_generic_code() {
    let (sender, sent) = erased();
    assert_eq!(TxSender::account_id(&sender), "alice.testnet");
    assert_eq!(TxSender::chain_id(&sender), "testnet");

    let response = bump(&sender).await.unwrap();
    assert_eq!(response.sender_id, "alice.testnet");
    assert_eq!(method_names(&sent.lock().unwrap()), ["bump"]);
}

#[tokio::test]
async fn an_erased_sender_drives_a_contract_call() {
    let (sender, sent) = erased();
    Counter::client(sender, counter())
        .increment(2)
        .await
        .unwrap();
    let sent = sent.lock().unwrap();
    let [(receiver_id, actions)] = sent.as_slice() else {
        panic!("expected one transaction, got {sent:?}");
    };
    assert_eq!(*receiver_id, counter());
    let [Action::FunctionCall(call)] = actions.as_slice() else {
        panic!("expected one function call, got {actions:?}");
    };
    assert_eq!(call.method_name, "increment");
    assert_eq!(call.args, br#"{"by":2}"#);
}

#[tokio::test]
async fn an_erased_sender_builds_a_transaction() {
    let (sender, sent) = erased();
    sender
        .transaction(counter())
        .add_action(FunctionCall::new("bump").args(json!({ "by": 1 })))
        .add_action(FunctionCall::new("bump").args(json!({ "by": 2 })))
        .await
        .unwrap();
    assert_eq!(method_names(&sent.lock().unwrap()), ["bump", "bump"]);
}

#[tokio::test]
async fn an_owned_erased_transaction_spawns() {
    let (sender, sent) = erased();
    let tx = Tx::new(Arc::clone(&sender), counter()).add_action(FunctionCall::new("bump"));
    tokio::spawn(tx.into_future()).await.unwrap().unwrap();
    assert_eq!(method_names(&sent.lock().unwrap()), ["bump"]);
}

#[tokio::test]
async fn errors_downcast_to_the_senders_own() {
    let (sender, sent) = erased();

    let refused = sender
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await
        .unwrap_err();
    assert!(
        matches!(
            refused.downcast_ref::<RecorderError>(),
            Some(RecorderError::Refused)
        ),
        "{refused:?}"
    );

    // An encoding error never reaches the sender: `Tx` converts it into the
    // erased error itself, so it downcasts to `CodecError`, not to the
    // recorder's `Codec` variant.
    let unencodable = std::collections::BTreeMap::from([((1, 2), 3)]);
    let codec = sender
        .transaction(counter())
        .add_action(FunctionCall::new("init").args(unencodable))
        .await
        .unwrap_err();
    assert!(codec.downcast_ref::<CodecError>().is_some(), "{codec:?}");
    assert!(sent.lock().unwrap().is_empty());
}

/// A `SigningNear` at any wait level erases into the same type.
#[allow(dead_code)]
fn signing_near_erases(alice: SigningNear) -> [ArcTxSender; 2] {
    use near_kit::transaction::Included;
    [Arc::new(alice.wait_until::<Included>()), Arc::new(alice)]
}
