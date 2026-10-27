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

/// A `Viewer` that isn't `Clone`.
struct Unclonable;

impl Viewer for Unclonable {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        Ok(ViewFunctionResult::new(b"3".to_vec(), 1, CryptoHash::ZERO))
    }
}

/// An extension-trait helper can return the view itself, so callers can
/// still pick a block or ask for metadata.
trait CounterExt: Viewer + Clone + Sized {
    fn count(&self) -> near_kit_primitives::transaction::View<Self, u64> {
        Counter::client(self.clone(), account!("c.near")).get_count()
    }
}

impl CounterExt for Backend {}

#[test]
fn views_and_calls_own_their_backend() {
    let backend = Backend::default();
    assert_eq!(run(backend.count().at(Finality::Optimistic)).unwrap(), 7);

    // The futures are 'static, so they can be spawned.
    fn assert_spawnable<F: std::future::Future + Send + 'static>(future: F) -> F {
        future
    }
    let counter = Counter::client(backend.clone(), account!("c.near"));
    let view = assert_spawnable(counter.get_count().into_future());
    let call = assert_spawnable(counter.reset().into_future());
    drop(counter);
    assert_eq!(futures::executor::block_on(view).unwrap(), 7);
    futures::executor::block_on(call).unwrap();

    // A backend that isn't `Clone` serves several clients through an `Arc`.
    let shared = Arc::new(Unclonable);
    let first = Counter::client(shared.clone(), account!("a.near"));
    let second = Counter::client(shared, account!("b.near"));
    assert_eq!(run(first.get_count()).unwrap(), 3);
    assert_eq!(run(second.get_count()).unwrap(), 3);

    // Or by reference, reading with `with_metadata`.
    let unclonable = Unclonable;
    let borrowed = Counter::client(&unclonable, account!("c.near"));
    let read = futures::executor::block_on(borrowed.get_count().with_metadata()).unwrap();
    assert_eq!((read.result, read.block_height), (3, 1));
}
