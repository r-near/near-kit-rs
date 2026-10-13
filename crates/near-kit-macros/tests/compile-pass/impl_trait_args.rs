//! Test that `impl Trait` arguments are accepted, as in any trait method.

use near_kit::transaction::{CallBuilder, FunctionCall};
use near_kit::*;

#[near_kit::contract]
pub trait Registry {
    fn lookup(&self, args: impl serde::Serialize) -> Option<String>;

    #[call]
    fn register(&mut self, args: impl serde::Serialize);
}

#[derive(serde::Serialize)]
struct Key {
    key: String,
}

fn main() {
    let near = Near::testnet().build();
    let client = RegistryClient::new(near, "registry.testnet".parse().unwrap());

    let _view = client.lookup(Key { key: "a".into() });
    let _call: CallBuilder = client.register(Key { key: "a".into() });
    let _offline: FunctionCall = Registry::register(Key { key: "a".into() });
}
