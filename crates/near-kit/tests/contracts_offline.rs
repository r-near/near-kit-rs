//! Compile tests for `#[near_kit::contract]` with `contracts` but without
//! `rpc`: the offline `FunctionCall` constructors must be generated, the
//! `Near`-backed client must not be.
//!
//! Run with:
//! `cargo test -p near-kit --no-default-features --features contracts --test contracts_offline`
#![cfg(all(
    feature = "contracts",
    not(feature = "rpc"),
    not(target_arch = "wasm32")
))]

#[test]
fn offline_contract_bindings() {
    let t = trybuild::TestCases::new();
    t.pass("tests/contracts-offline/pass/*.rs");
    t.compile_fail("tests/contracts-offline/fail/*.rs");
}
