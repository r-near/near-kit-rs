//! `contracts` without `rpc`: the macro, its `#[call]` / `#[json]` / `#[borsh]`
//! markers, and the offline `FunctionCall` constructors all work.

use near_kit::protocol::{Action, FunctionCallAction};
use near_kit::transaction::FunctionCall;
use near_kit::{Gas, NearToken};

#[derive(serde::Serialize)]
pub struct AddArgs {
    pub value: u64,
}

#[derive(borsh::BorshSerialize)]
pub struct UploadArgs {
    pub data: Vec<u8>,
}

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);

    #[call]
    fn add(&mut self, args: AddArgs);

    #[call]
    #[borsh]
    fn upload(&mut self, args: UploadArgs);
}

#[near_kit::contract(borsh)]
pub trait Store {
    #[json]
    fn get_status(&self) -> String;

    #[call]
    #[json]
    fn set_status(&mut self, args: AddArgs);

    #[call]
    fn clear(&mut self);
}

fn main() {
    let call: FunctionCall = Counter::add(AddArgs { value: 5 })
        .gas(Gas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1));
    let action: FunctionCallAction = call.try_into().unwrap();
    assert_eq!(action.method_name, "add");
    assert_eq!(action.args, br#"{"value":5}"#);

    let action: FunctionCallAction = Counter::increment().try_into().unwrap();
    assert_eq!(action.args, b"{}");

    let action: FunctionCallAction = Counter::upload(UploadArgs { data: vec![1, 2] })
        .try_into()
        .unwrap();
    assert_eq!(action.args, [2, 0, 0, 0, 1, 2]);

    let action: FunctionCallAction = Store::set_status(AddArgs { value: 1 }).try_into().unwrap();
    assert_eq!(action.args, br#"{"value":1}"#);

    let action: Action = Store::clear().into_action().unwrap();
    let _ = action;
}
