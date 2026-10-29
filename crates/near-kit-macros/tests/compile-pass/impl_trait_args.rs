//! Test that `impl Trait` arguments are accepted, as in any trait method.

use near_kit::transaction::{Call, FunctionCall};
use near_kit::signer::{InMemorySigner, SecretKey};
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
    let client = near.contract::<Registry>(account!("registry.testnet"));

    let _view = client.lookup(Key { key: "a".into() });
    let account: SigningNear = near.with_signer(
        InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519()),
    );
    let signing_client = account.contract::<Registry>(account!("registry.testnet"));
    let _call: Call<SigningNear> = signing_client.register(Key { key: "a".into() });
    let _offline: FunctionCall = Registry::register(Key { key: "a".into() });
}
