//! A view method on a client whose backend can't view reports the `Viewer`
//! diagnostic.

use near_kit::protocol::{Action, ChainId};
use near_kit::rpc::SendTxResponse;
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

#[derive(Clone)]
struct SendOnly(AccountId);

impl TxSender for SendOnly {
    type Output = SendTxResponse;
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        &self.0
    }

    fn chain_id(&self) -> &ChainId {
        unreachable!("the test only compiles")
    }

    async fn send(&self, _receiver: AccountId, _: Vec<Action>) -> Result<SendTxResponse, String> {
        Ok(SendTxResponse::new(CryptoHash::ZERO, self.0.clone(), None))
    }
}

async fn run() {
    let counter = Counter::client(SendOnly(account!("me.near")), account!("counter.near"));
    let _ = counter.increment().await;
    let _ = counter.get_count().await;
}

fn main() {
    let _ = run();
}
