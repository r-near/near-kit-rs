//! Test that Borsh contract macro generates valid code.
//!
//! Note: We use types that implement BorshSerialize/BorshDeserialize from near_kit's
//! re-exports to avoid needing a direct borsh dependency in the test.

use near_kit::transaction::{Call, View};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::*;

/// Borsh contract using primitive types that already implement Borsh traits
#[near_kit::contract(borsh)]
pub trait BorshContract {
    fn get_value(&self) -> u64;
    fn get_flag(&self) -> bool;

    #[call]
    fn set_value(&mut self);
}

fn main() {
    let near = Near::testnet().build();
    let client: BorshContractClient = near.contract::<BorshContract>(account!("contract.testnet"));

    let _view: View<Near, u64> = client.get_value();
    let _view: View<Near, bool> = client.get_flag();
    let account: SigningNear = near.with_signer(
        InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519()),
    );
    let signing_client: BorshContractClient<SigningNear> =
        account.contract::<BorshContract>(account!("contract.testnet"));
    let _call: Call<SigningNear> = signing_client.set_value();
}
