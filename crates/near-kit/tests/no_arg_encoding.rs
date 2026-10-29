//! The bytes each way of making a view or call without arguments sends.
//!
//! JSON sends the empty object `{}` however the view or call is built, so a
//! method whose parameters are all `Option` gets the object it needs. Borsh
//! sends no bytes, the Borsh encoding of `()`.
#![cfg(feature = "contracts")]

use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::transaction::FunctionCall;
use near_kit::{AccountId, Contract, CryptoHash, TxSender, View, Viewer, account};

#[near_kit::contract]
pub trait JsonContract {
    fn get(&self) -> Option<u64>;

    #[call]
    fn run(&mut self);
}

#[near_kit::contract(borsh)]
pub trait BorshContract {
    fn get(&self) -> Option<u64>;

    #[json]
    fn get_json(&self) -> Option<u64>;

    #[call]
    fn run(&mut self);

    #[call]
    #[json]
    fn run_json(&mut self);
}

/// Records the arguments of every view and call, and answers views with an
/// empty result.
#[derive(Clone, Default)]
struct Recorder {
    args: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Recorder {
    fn last(&self) -> Vec<u8> {
        self.args.lock().unwrap().pop().expect("something was sent")
    }
}

impl Viewer for Recorder {
    type ViewError = std::convert::Infallible;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, Self::ViewError> {
        self.args.lock().unwrap().push(args.to_vec());
        // A Borsh `None`, which JSON reads as `null` from an empty result.
        let result = if args.is_empty() { vec![0] } else { Vec::new() };
        Ok(ViewFunctionResult::new(result, 1, CryptoHash::ZERO))
    }
}

impl TxSender for Recorder {
    type Output = ();
    type SendError = std::convert::Infallible;

    fn account_id(&self) -> &AccountId {
        unreachable!("not used by Call")
    }

    async fn send(
        &self,
        _receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<(), Self::SendError> {
        for action in actions {
            let Action::FunctionCall(call) = action else {
                panic!("expected a function call, got {action:?}");
            };
            self.args.lock().unwrap().push(call.args);
        }
        Ok(())
    }
}

fn contract() -> AccountId {
    account!("contract.near")
}

fn action_args(call: FunctionCall) -> Vec<u8> {
    match call.into_action().unwrap() {
        Action::FunctionCall(call) => call.args,
        other => panic!("expected a function call, got {other:?}"),
    }
}

#[tokio::test]
async fn json_views_without_arguments_send_an_empty_object() {
    let recorder = Recorder::default();

    let client = JsonContract::client(recorder.clone(), contract());
    assert_eq!(client.get().await.unwrap(), None);
    assert_eq!(recorder.last(), b"{}", "#[contract] view");

    let client = BorshContract::client(recorder.clone(), contract());
    assert_eq!(client.get_json().await.unwrap(), None);
    assert_eq!(recorder.last(), b"{}", "#[json] view on a Borsh contract");

    let value: Option<u64> = View::json(recorder.clone(), contract(), "get")
        .await
        .unwrap();
    assert_eq!(value, None);
    assert_eq!(recorder.last(), b"{}", "View::json");
}

#[tokio::test]
async fn json_calls_without_arguments_send_an_empty_object() {
    let recorder = Recorder::default();

    assert_eq!(
        action_args(FunctionCall::new("run")),
        b"{}",
        "FunctionCall::new"
    );
    assert_eq!(
        action_args(JsonContract::run()),
        b"{}",
        "#[contract] constructor"
    );
    assert_eq!(
        action_args(BorshContract::run_json()),
        b"{}",
        "#[json] constructor on a Borsh contract"
    );

    JsonContract::client(recorder.clone(), contract())
        .run()
        .await
        .unwrap();
    assert_eq!(recorder.last(), b"{}", "#[contract] call");
}

#[tokio::test]
async fn borsh_views_and_calls_without_arguments_send_nothing() {
    let recorder = Recorder::default();

    let client = BorshContract::client(recorder.clone(), contract());
    assert_eq!(client.get().await.unwrap(), None);
    assert_eq!(recorder.last(), b"", "#[contract(borsh)] view");

    let value: Option<u64> = View::borsh(recorder.clone(), contract(), "get")
        .await
        .unwrap();
    assert_eq!(value, None);
    assert_eq!(recorder.last(), b"", "View::borsh");

    assert_eq!(
        action_args(BorshContract::run()),
        b"",
        "#[contract(borsh)] constructor"
    );
    client.run().await.unwrap();
    assert_eq!(recorder.last(), b"", "#[contract(borsh)] call");
}
