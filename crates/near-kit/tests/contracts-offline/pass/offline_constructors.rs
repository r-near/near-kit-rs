//! `contracts` without `rpc`: the macro, its `#[call]` / `#[json]` / `#[borsh]`
//! markers, the offline `FunctionCall` constructors and the backend-generic
//! client all work.

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

    fn lookup(&self, args: impl serde::Serialize) -> Option<u64>;

    #[call]
    fn register(&mut self, args: impl serde::Serialize);
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

/// A backend that can view but not send.
struct Fixed;

impl near_kit::Viewer for Fixed {
    type ViewError = std::convert::Infallible;

    async fn view_function(
        &self,
        _contract_id: &near_kit::AccountId,
        _method: &str,
        _args: &[u8],
        _block: near_kit::rpc::BlockReference,
    ) -> Result<near_kit::rpc::ViewFunctionResult, Self::ViewError> {
        Ok(near_kit::rpc::ViewFunctionResult::new(b"3".to_vec(), 1, near_kit::CryptoHash::ZERO))
    }
}

fn main() {
    use near_kit::Contract;

    let counter: CounterClient<Fixed> =
        Counter::client(Fixed, near_kit::account!("counter.near"));
    let _view: near_kit::transaction::View<'_, Fixed, u64> = counter.get_count();

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

    let action: FunctionCallAction = Counter::register(AddArgs { value: 7 }).try_into().unwrap();
    assert_eq!(action.args, br#"{"value":7}"#);

    let action: FunctionCallAction = Store::set_status(AddArgs { value: 1 }).try_into().unwrap();
    assert_eq!(action.args, br#"{"value":1}"#);

    let action: Action = Store::clear().into_action().unwrap();
    let _ = action;
}
