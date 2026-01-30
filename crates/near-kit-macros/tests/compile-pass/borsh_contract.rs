//! Test that Borsh contract macro generates valid code.
//!
//! Note: We use types that implement BorshSerialize/BorshDeserialize from near_kit's
//! re-exports to avoid needing a direct borsh dependency in the test.

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
    // Verify the generated client can be constructed
    let near = Near::testnet().build();
    let client = BorshContractClient::new(&near, "contract.testnet".parse().unwrap());

    // Verify methods exist and have correct return types
    // Borsh view methods should return ViewCallBorsh, not ViewCall
    let _view: ViewCallBorsh<u64> = client.get_value();
    let _view: ViewCallBorsh<bool> = client.get_flag();
    let _call: CallBuilder = client.set_value();
}
