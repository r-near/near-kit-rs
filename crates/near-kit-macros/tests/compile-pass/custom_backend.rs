//! A generated client works over any `Viewer`/`TxSender`, not just `Near`,
//! and its futures stay `Send` for generic backends. Backends can also sit
//! behind `&`, `Box` and `Arc`, including as trait objects.

use std::sync::Arc;

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

#[derive(Clone)]
struct Mock(AccountId);

impl Viewer for Mock {
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

impl TxSender for Mock {
    type Output = ();
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        &self.0
    }

    async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
        Ok(())
    }
}

async fn bump<C>(counter: &CounterClient<C>) -> Result<u64, ContractError<C::ViewError>>
where
    C: Viewer + TxSender + Clone + 'static,
    C::ViewError: 'static,
    C::Output: 'static,
    C::SendError: 'static,
{
    let _ = counter.increment().await;
    counter.get_count().await
}

/// A user's object-safe sender, used as `Arc<dyn UserSender>`.
trait UserSender: Send + Sync {
    fn id(&self) -> &AccountId;
}

impl UserSender for Mock {
    fn id(&self) -> &AccountId {
        &self.0
    }
}

impl TxSender for dyn UserSender {
    type Output = ();
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        self.id()
    }

    async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
        Ok(())
    }
}

fn assert_send<F: std::future::Future + Send>(_: F) {}

fn main() {
    let counter: CounterClient<Mock> = Counter::client(Mock(account!("me.near")), account!("c.near"));
    assert_send(bump(&counter));

    let shared: Arc<dyn UserSender> = Arc::new(Mock(account!("me.near")));
    let counter = Counter::client(shared, account!("c.near"));
    assert_send(async move {
        let _ = counter.increment().await;
    });
}
