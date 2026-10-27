//! Test that mixed format contracts (per-method overrides) generate valid code.
//!
//! Note: We use primitive types to avoid needing external borsh/serde derives.

use near_kit::transaction::{Call, FunctionCall, View};
use near_kit::*;

/// Mixed contract - JSON default with Borsh overrides
#[near_kit::contract]
pub trait MixedContract {
    // JSON view (default)
    fn get_json_value(&self) -> u64;

    // Borsh view (override)
    #[borsh]
    fn get_borsh_value(&self) -> u64;

    // JSON call (default)
    #[call]
    fn set_json_value(&mut self);

    // Borsh call (override)
    #[call]
    #[borsh]
    fn set_borsh_value(&mut self);
}

/// Mixed contract - Borsh default with JSON overrides
#[near_kit::contract(borsh)]
pub trait MixedContractBorshDefault {
    // Borsh view (default)
    fn get_borsh_value(&self) -> u64;

    // JSON view (override)
    #[json]
    fn get_json_value(&self) -> u64;

    // Borsh call (default)
    #[call]
    fn set_borsh_value(&mut self);

    // JSON call (override)
    #[call]
    #[json]
    fn set_json_value(&mut self);
}

fn main() {
    let near = Near::testnet().build();

    {
        let client = near.contract::<MixedContract>(account!("contract.testnet"));
        let _: View<'_, Near, u64> = client.get_json_value();
        let _: View<'_, Near, u64> = client.get_borsh_value();
        let _: Call<'_, Near> = client.set_json_value();
        let _: Call<'_, Near> = client.set_borsh_value();
    }

    {
        let client = near.contract::<MixedContractBorshDefault>(account!("contract.testnet"));
        let _: View<'_, Near, u64> = client.get_borsh_value();
        let _: View<'_, Near, u64> = client.get_json_value();
        let _: Call<'_, Near> = client.set_borsh_value();
        let _: Call<'_, Near> = client.set_json_value();
    }

    // Static FunctionCall constructors (call methods only, not view)
    let _: FunctionCall = MixedContract::set_json_value();
    let _: FunctionCall = MixedContract::set_borsh_value();
    let _: FunctionCall = MixedContractBorshDefault::set_borsh_value();
    let _: FunctionCall = MixedContractBorshDefault::set_json_value();
}
