//! `#[contract]` through near-kit-primitives alone: a crate without near-kit
//! (and so without the RPC client) gets the offline `FunctionCall`
//! constructors and a client generic over any `Viewer` / `TxSender`.
#![cfg(feature = "contracts")]

use std::future::IntoFuture;
use std::sync::{Arc, Mutex};

use near_kit_primitives::protocol::Action;
use near_kit_primitives::rpc::{BlockReference, Finality, ViewFunctionResult};
use near_kit_primitives::transaction::{CodecError, ContractError};
use near_kit_primitives::{
    AccountId, Contract, CryptoHash, Gas, NearToken, TxSender, Viewer, account,
};
use serde::Serialize;

#[derive(Serialize)]
pub struct AddArgs {
    pub amount: u64,
}

#[near_kit_primitives::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn add(&mut self, args: AddArgs);

    #[call]
    fn reset(&mut self);
}

#[test]
fn offline_constructors_build_actions() {
    let action = Counter::add(AddArgs { amount: 5 })
        .gas(Gas::from_tgas(10))
        .deposit(NearToken::from_yoctonear(1))
        .into_action()
        .unwrap();
    match action {
        Action::FunctionCall(call) => {
            assert_eq!(call.method_name, "add");
            assert_eq!(call.args, br#"{"amount":5}"#);
            assert_eq!(call.gas, Gas::from_tgas(10));
            assert_eq!(call.deposit, NearToken::from_yoctonear(1));
        }
        other => panic!("expected a function call, got {other:?}"),
    }

    let action = Counter::reset().into_action().unwrap();
    assert!(
        matches!(action, Action::FunctionCall(call) if call.method_name == "reset" && call.args == b"{}")
    );
}

type Views = Vec<(String, Vec<u8>, BlockReference)>;
type Sent = Vec<(AccountId, Vec<Action>)>;

/// Records what it was asked (in state shared with the test) and answers
/// views with `7`.
#[derive(Clone, Default)]
struct Backend {
    views: Arc<Mutex<Views>>,
    sent: Arc<Mutex<Sent>>,
}

impl Viewer for Backend {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        self.views
            .lock()
            .unwrap()
            .push((method.to_owned(), args.to_vec(), block));
        Ok(ViewFunctionResult::new(b"7".to_vec(), 1, CryptoHash::ZERO))
    }
}

impl TxSender for Backend {
    type Output = ();
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        unreachable!("not used by generated clients")
    }

    async fn send(&self, receiver_id: AccountId, actions: Vec<Action>) -> Result<(), String> {
        self.sent.lock().unwrap().push((receiver_id, actions));
        Ok(())
    }
}

fn run<F: IntoFuture>(future: F) -> F::Output {
    futures::executor::block_on(future.into_future())
}

#[test]
fn generated_client_runs_on_any_backend() {
    let backend = Backend::default();
    let counter: CounterClient<Backend> = Counter::client(backend.clone(), account!("c.near"));
    assert_eq!(counter.contract_id(), &account!("c.near"));

    assert_eq!(run(counter.get_count()).unwrap(), 7);
    assert_eq!(
        run(counter.get_count().at(Finality::Optimistic)).unwrap(),
        7
    );
    run(counter
        .add(AddArgs { amount: 5 })
        .deposit(NearToken::from_yoctonear(1)))
    .unwrap();

    assert_eq!(
        *backend.views.lock().unwrap(),
        [
            (
                "get_count".to_owned(),
                b"{}".to_vec(),
                BlockReference::default()
            ),
            (
                "get_count".to_owned(),
                b"{}".to_vec(),
                BlockReference::Finality(Finality::Optimistic)
            ),
        ]
    );
    let expected: Action = Counter::add(AddArgs { amount: 5 })
        .deposit(NearToken::from_yoctonear(1))
        .try_into()
        .unwrap();
    assert_eq!(
        *backend.sent.lock().unwrap(),
        [(account!("c.near"), vec![expected])]
    );
}

#[test]
fn generated_view_decode_errors_are_codec_errors() {
    #[near_kit_primitives::contract]
    trait Wrong {
        fn get_count(&self) -> String;
    }

    let wrong = Wrong::client(Backend::default(), account!("c.near"));
    assert!(matches!(
        run(wrong.get_count()),
        Err(ContractError::Codec(CodecError::Json(_)))
    ));
}
