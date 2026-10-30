//! `#[call(gas = .., deposit = ..)]`: the defaults a call is built with,
//! through both the client method and the `FunctionCall` constructor, and
//! how the call site overrides them.
#![cfg(feature = "contracts")]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use borsh::BorshSerialize;
use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_kit::{AccountId, Contract, Gas, NearToken, TxSender, account};
use serde::Serialize;

const TRANSFER_CALL_GAS: Gas = Gas::from_tgas(300);
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

#[derive(Serialize, BorshSerialize)]
pub struct Transfer {
    pub amount: u64,
}

/// Counts its calls, so a test can see when a default is evaluated.
static EVALUATED: AtomicU64 = AtomicU64::new(0);

fn counted_gas() -> Gas {
    Gas::from_tgas(10 + EVALUATED.fetch_add(1, Ordering::SeqCst))
}

#[near_kit::contract]
pub trait Token {
    #[call(deposit = ONE_YOCTO)]
    fn transfer(&mut self, #[args] args: Transfer);

    #[call(gas = TRANSFER_CALL_GAS, deposit = NearToken::from_yoctonear(1))]
    fn transfer_call(&mut self, #[args] args: Transfer);

    #[call(deposit = NearToken::from_millinear(5), gas = Gas::from_tgas(50))]
    fn register(&mut self);

    #[call(gas = counted_gas())]
    fn counted(&mut self);

    #[call]
    fn plain(&mut self, #[args] args: Transfer);
}

#[near_kit::contract(borsh)]
pub trait BorshToken {
    #[call(gas = Gas::from_tgas(40), deposit = ONE_YOCTO)]
    fn transfer(&mut self, #[args] args: Transfer);
}

/// Records the gas and deposit of every function call it's asked to send.
#[derive(Clone, Default)]
struct Recorder {
    sent: Arc<Mutex<Vec<(Gas, NearToken)>>>,
}

impl Recorder {
    fn last(&self) -> (Gas, NearToken) {
        self.sent.lock().unwrap().pop().expect("something was sent")
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
            self.sent.lock().unwrap().push((call.gas, call.deposit));
        }
        Ok(())
    }
}

fn gas_and_deposit(call: FunctionCall) -> (Gas, NearToken) {
    match call.into_action().unwrap() {
        Action::FunctionCall(call) => (call.gas, call.deposit),
        other => panic!("expected a function call, got {other:?}"),
    }
}

fn transfer() -> Transfer {
    Transfer { amount: 1 }
}

#[test]
fn constructors_start_from_the_declared_defaults() {
    assert_eq!(
        gas_and_deposit(Token::transfer(transfer())),
        (Gas::from_tgas(30), ONE_YOCTO),
        "deposit only: gas stays 30 Tgas"
    );
    assert_eq!(
        gas_and_deposit(Token::transfer_call(transfer())),
        (TRANSFER_CALL_GAS, ONE_YOCTO)
    );
    assert_eq!(
        gas_and_deposit(Token::register()),
        (Gas::from_tgas(50), NearToken::from_millinear(5)),
        "keys in either order, method without an argument"
    );
    assert_eq!(
        gas_and_deposit(BorshToken::transfer(transfer())),
        (Gas::from_tgas(40), ONE_YOCTO)
    );
    assert_eq!(
        gas_and_deposit(Token::plain(transfer())),
        (Gas::from_tgas(30), NearToken::ZERO),
        "bare #[call] keeps FunctionCall's defaults"
    );
}

#[test]
fn a_function_call_overrides_the_defaults() {
    assert_eq!(
        gas_and_deposit(Token::transfer_call(transfer()).gas(Gas::from_tgas(100))),
        (Gas::from_tgas(100), ONE_YOCTO)
    );
    assert_eq!(
        gas_and_deposit(Token::transfer_call(transfer()).deposit(NearToken::ZERO)),
        (TRANSFER_CALL_GAS, NearToken::ZERO)
    );
}

#[tokio::test]
async fn client_calls_send_the_defaults_and_the_call_overrides_them() {
    let recorder = Recorder::default();
    let token = Token::client(recorder.clone(), account!("token.near"));

    token.transfer(transfer()).await.unwrap();
    assert_eq!(recorder.last(), (Gas::from_tgas(30), ONE_YOCTO));

    token.transfer_call(transfer()).await.unwrap();
    assert_eq!(recorder.last(), (TRANSFER_CALL_GAS, ONE_YOCTO));

    token
        .transfer_call(transfer())
        .gas(Gas::from_tgas(100))
        .await
        .unwrap();
    assert_eq!(recorder.last(), (Gas::from_tgas(100), ONE_YOCTO));

    token
        .transfer(transfer())
        .deposit(NearToken::from_yoctonear(2))
        .await
        .unwrap();
    assert_eq!(
        recorder.last(),
        (Gas::from_tgas(30), NearToken::from_yoctonear(2))
    );

    BorshToken::client(recorder.clone(), account!("token.near"))
        .transfer(transfer())
        .await
        .unwrap();
    assert_eq!(recorder.last(), (Gas::from_tgas(40), ONE_YOCTO));

    // A `Call` turned into a `FunctionCall` for batching keeps them.
    let call = FunctionCall::from(token.transfer_call(transfer()));
    assert_eq!(gas_and_deposit(call), (TRANSFER_CALL_GAS, ONE_YOCTO));
}

#[tokio::test]
async fn defaults_are_evaluated_each_time_a_call_is_built() {
    let recorder = Recorder::default();
    let token = Token::client(recorder.clone(), account!("token.near"));
    let start = EVALUATED.load(Ordering::SeqCst);

    let first = gas_and_deposit(Token::counted()).0;
    token.counted().await.unwrap();
    let second = recorder.last().0;

    assert_eq!(EVALUATED.load(Ordering::SeqCst), start + 2);
    assert_eq!(second, first.checked_add(Gas::from_tgas(1)).unwrap());
}
