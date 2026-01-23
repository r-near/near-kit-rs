//! Integration tests for near-kit.
//!
//! These tests run against a local NEAR sandbox and require the `sandbox` feature.
//!
//! Run with: `cargo test --features sandbox --test integration`

#![cfg(feature = "sandbox")]

mod basic_integration;
mod debug_rpc_responses;
mod delegate_action_integration;
mod global_contracts_integration;
mod rpc_types_integration;
mod sandbox_integration;
mod typed_contract_integration;
