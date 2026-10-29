//! A `#[call]` method on a client whose backend can't send transactions
//! reports the `TxSender` diagnostic, not "remove the `.await`".

use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

#[derive(Clone)]
struct ViewOnly;

impl Viewer for ViewOnly {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        Err("offline".to_owned())
    }
}

async fn run() {
    let counter = Counter::client(ViewOnly, account!("counter.near"));
    let _ = counter.get_count().await;
    let _ = counter.increment().await;
}

fn main() {
    let _ = run();
}
