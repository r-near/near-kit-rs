//! `#[contract]` through near-kit-primitives alone: a crate without near-kit
//! (and so without the RPC client) gets the offline `FunctionCall`
//! constructors.
#![cfg(feature = "contracts")]

use near_kit_primitives::protocol::Action;
use near_kit_primitives::{Gas, NearToken};
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
