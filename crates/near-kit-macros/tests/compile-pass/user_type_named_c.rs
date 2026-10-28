//! Test that a contract's own types can use names a generated generic might,
//! such as `C`.

use near_kit::transaction::{Call, View};
use near_kit::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct C {
    value: u64,
}

#[near_kit::contract]
pub trait Store {
    fn get(&self, args: C) -> C;

    #[call]
    fn set(&mut self, args: C);
}

fn main() {
    let near = Near::testnet().build();
    let client = near.contract::<Store>(account!("store.testnet"));
    let _view: View<Near, C> = client.get(C { value: 1 });
    let _call: fn(&StoreClient<SigningNear>, C) -> Call<SigningNear> = StoreClient::set;
}
