//! A view method on a client whose backend can't view reports the `Viewer`
//! diagnostic.

use near_kit::protocol::Action;
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

struct SendOnly(AccountId);

impl TxSender for SendOnly {
    type Output = ();
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        &self.0
    }

    async fn send(&self, _receiver_id: AccountId, _actions: Vec<Action>) -> Result<(), String> {
        Ok(())
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
