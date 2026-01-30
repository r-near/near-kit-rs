//! Test that mixed format contracts (per-method overrides) generate valid code.
//!
//! Note: We use primitive types to avoid needing external borsh/serde derives.

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

    // Test MixedContract (JSON default)
    {
        let client = MixedContractClient::new(&near, "contract.testnet".parse().unwrap());

        // JSON methods return ViewCall
        let _: ViewCall<u64> = client.get_json_value();
        let _: CallBuilder = client.set_json_value();

        // Borsh-overridden methods return ViewCallBorsh
        let _: ViewCallBorsh<u64> = client.get_borsh_value();
        let _: CallBuilder = client.set_borsh_value();
    }

    // Test MixedContractBorshDefault (Borsh default)
    {
        let client =
            MixedContractBorshDefaultClient::new(&near, "contract.testnet".parse().unwrap());

        // Borsh methods return ViewCallBorsh
        let _: ViewCallBorsh<u64> = client.get_borsh_value();
        let _: CallBuilder = client.set_borsh_value();

        // JSON-overridden methods return ViewCall
        let _: ViewCall<u64> = client.get_json_value();
        let _: CallBuilder = client.set_json_value();
    }
}
