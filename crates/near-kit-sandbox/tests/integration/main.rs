//! Docker-backed integration tests for near-kit.
//!
//! These tests run against a local NEAR sandbox and require the
//! `integration-tests` feature.
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

mod delegate_action_integration;
mod delegate_v2_integration;
mod error_consolidation_integration;
mod error_handling_integration;
mod exec_metadata_integration;
mod gas_key_transaction_integration;
mod global_contracts_integration;
mod ml_dsa_integration;
mod offline_signing_integration;
mod rpc_types_integration;
mod sandbox_integration;
mod signer_edge_cases_integration;
mod stabilized_rpc_integration;
mod token_error_integration;
mod token_integration;
#[cfg(feature = "tracing")]
mod tracing_integration;
mod typed_contract_integration;
mod typed_error_integration;
mod view_state_integration;

mod support;
