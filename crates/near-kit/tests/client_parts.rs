//! Every contract client hands out the backend and contract account it was
//! built from: `#[contract]` clients and the token clients alike, whether
//! built from a `Near` or a `SigningNear`.
#![cfg(all(feature = "contracts", feature = "rpc"))]

use std::sync::Arc;

use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::standards::{FungibleToken, NonFungibleToken};
use near_kit::*;

#[near_kit::contract]
pub trait Counter {
    fn get_num(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

fn near() -> Near {
    Near::testnet().build()
}

fn alice(near: &Near) -> SigningNear {
    near.with_signer(InMemorySigner::new(
        account!("alice.testnet"),
        SecretKey::generate_ed25519(),
    ))
}

/// Same RPC client (clones share it), so the same `Near`.
fn assert_same_near(got: &Near, want: &Near) {
    assert!(std::ptr::eq(got.rpc(), want.rpc()));
    assert_eq!(got.chain_id(), want.chain_id());
}

/// Same account, signer and RPC client, so the same `SigningNear`.
fn assert_same_sender(got: &SigningNear, want: &SigningNear) {
    assert_eq!(got.account_id(), want.account_id());
    assert!(Arc::ptr_eq(got.signer(), want.signer()));
    assert_same_near(got.near(), want.near());
}

#[test]
fn generated_clients_hand_out_their_parts() {
    let near = near();
    let alice = alice(&near);
    let id = account!("counter.testnet");

    let client = near.contract::<Counter>(&id);
    assert_same_near(client.backend(), &near);
    let (backend, contract_id) = client.into_parts();
    assert_same_near(&backend, &near);
    assert_eq!(contract_id, id);

    let client = alice.contract::<Counter>(&id);
    assert_same_sender(client.backend(), &alice);
    let (backend, contract_id) = client.into_parts();
    assert_same_sender(&backend, &alice);
    assert_eq!(contract_id, id);

    // The parts build the same client again.
    let rebuilt: CounterClient<SigningNear> = Counter::client(backend, contract_id);
    assert_same_sender(rebuilt.backend(), &alice);
    assert_eq!(rebuilt.contract_id(), &id);
}

#[test]
fn fungible_token_clients_hand_out_their_parts() {
    let near = near();
    let alice = alice(&near);
    let id = account!("wrap.testnet");

    let token = near.contract::<FungibleToken>(&id);
    assert_same_near(token.backend(), &near);
    let (backend, contract_id) = token.into_parts();
    assert_same_near(&backend, &near);
    assert_eq!(contract_id, id);

    let token = alice.contract::<FungibleToken>(&id);
    assert_same_sender(token.backend(), &alice);
    let (backend, contract_id) = token.into_parts();
    assert_same_sender(&backend, &alice);
    assert_eq!(contract_id, id);
}

#[test]
fn non_fungible_token_clients_hand_out_their_parts() {
    let near = near();
    let alice = alice(&near);
    let id = account!("nft.testnet");

    let nft = near.contract::<NonFungibleToken>(&id);
    assert_same_near(nft.backend(), &near);
    let (backend, contract_id) = nft.into_parts();
    assert_same_near(&backend, &near);
    assert_eq!(contract_id, id);

    let nft = alice.contract::<NonFungibleToken>(&id);
    assert_same_sender(nft.backend(), &alice);
    let (backend, contract_id) = nft.into_parts();
    assert_same_sender(&backend, &alice);
    assert_eq!(contract_id, id);
}
