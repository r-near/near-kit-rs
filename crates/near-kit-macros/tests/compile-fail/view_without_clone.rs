//! A view needs its own copy of the backend; a backend that isn't `Clone`
//! gets one plain `Clone` error (share it with `Arc` or `&` instead).

use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;
}

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
        Err("offline".to_owned())
    }
}

async fn run() {
    let counter = Counter::client(Unclonable, account!("counter.near"));
    let _ = counter.get_count().await;
}

fn main() {
    let _ = run();
}
