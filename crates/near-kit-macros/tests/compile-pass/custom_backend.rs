//! A generated client works over any `Viewer`/`TxSender`, not just `Near`,
//! and its futures stay `Send` for generic backends.

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::transaction::ContractError;
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

struct Mock(AccountId);

impl Viewer for Mock {
    type Error = String;

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

impl TxSender for Mock {
    type Output = ();
    type Error = String;

    fn account_id(&self) -> &AccountId {
        &self.0
    }

    async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
        Ok(())
    }
}

async fn bump<C>(counter: &CounterClient<C>) -> Result<u64, ContractError<<C as Viewer>::Error>>
where
    C: Viewer + TxSender,
{
    let _ = counter.increment().await;
    counter.get_count().await
}

fn assert_send<F: std::future::Future + Send>(_: F) {}

fn main() {
    let counter: CounterClient<Mock> = Counter::handle(Mock(account!("me.near")), account!("c.near"));
    assert_send(bump(&counter));
}
