//! Integration tests for typed contract macro error paths.
//!
//! Tests error handling for `#[near_kit::contract]` macro-generated clients.
//! Run with: `cargo test --features sandbox --test integration typed_contract_error`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;
use serde::{Deserialize, Serialize};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tcerr{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// ============================================================================
// Define test contract interfaces
// ============================================================================

/// A message in the guestbook.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuestbookMessage {
    pub premium: bool,
    pub sender: String,
    pub text: String,
}

/// Arguments for adding a message.
#[derive(Debug, Clone, Serialize)]
pub struct AddMessageArgs {
    pub text: String,
}

/// Typed interface for the guestbook contract.
#[near_kit::contract]
pub trait Guestbook {
    /// Get the total number of messages.
    fn total_messages(&self) -> u32;

    /// Get all messages.
    fn get_messages(&self) -> Vec<GuestbookMessage>;

    /// Add a new message to the guestbook.
    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
}

// ============================================================================
// Error Tests
// ============================================================================

#[tokio::test]
async fn test_typed_contract_view_on_nonexistent_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to call view method on non-existent contract
    let guestbook = near.contract::<dyn Guestbook>("nonexistent-contract.sandbox");
    let result = guestbook.total_messages().await;

    assert!(result.is_err(), "Should error for non-existent account");
    let err = result.unwrap_err();
    println!("View on non-existent account: {:?}", err);

    // Should be AccountNotFound or ContractNotDeployed
    match err {
        Error::Rpc(RpcError::AccountNotFound(_)) => { /* Expected */ }
        Error::Rpc(RpcError::ContractNotDeployed(_)) => { /* Also acceptable */ }
        _ => panic!(
            "Expected AccountNotFound or ContractNotDeployed, got: {:?}",
            err
        ),
    }
}

#[tokio::test]
async fn test_typed_contract_view_on_account_without_contract() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create an account without any contract
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to call view method on account without contract
    let guestbook = near.contract::<dyn Guestbook>(&account_id);
    let result = guestbook.total_messages().await;

    assert!(result.is_err(), "Should error for account without contract");
    let err = result.unwrap_err();
    println!("View on account without contract: {:?}", err);

    match err {
        Error::Rpc(RpcError::ContractNotDeployed(_)) => { /* Expected */ }
        Error::Rpc(_) => { /* Other RPC errors acceptable */ }
        _ => panic!("Expected RPC error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_typed_contract_call_without_signer() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy a guestbook contract
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client WITHOUT a signer
    let no_signer_near = Near::custom(sandbox.rpc_url()).build();
    let guestbook = no_signer_near.contract::<dyn Guestbook>(&contract_id);

    // Try to call a mutating method without signer
    let result = guestbook
        .add_message(AddMessageArgs {
            text: "Test".to_string(),
        })
        .await;

    assert!(result.is_err(), "Should error when no signer configured");
    match result.unwrap_err() {
        Error::NoSigner => { /* Expected */ }
        e => panic!("Expected NoSigner, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_typed_contract_view_on_wrong_contract_type() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy the FT contract (not a guestbook)
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/fungible_token.wasm")
        .expect("fungible_token.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to use guestbook interface on FT contract
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);
    let result = guestbook.total_messages().await;

    assert!(result.is_err(), "Should error for wrong contract type");
    let err = result.unwrap_err();
    println!("Guestbook call on FT contract: {:?}", err);

    // Should be a contract execution error (method not found)
    match err {
        Error::Rpc(RpcError::ContractExecution { .. }) => { /* Expected */ }
        Error::Rpc(_) => { /* Other RPC errors acceptable */ }
        _ => panic!("Expected RPC error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_typed_contract_call_with_insufficient_gas() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Try to call with extremely low gas
    let result = guestbook
        .add_message(AddMessageArgs {
            text: "Test message".to_string(),
        })
        .gas(Gas::ggas(1)) // Way too little gas
        .await;

    assert!(result.is_err(), "Should error with insufficient gas");
    let err = result.unwrap_err();
    println!("Insufficient gas error: {:?}", err);

    // Should be a transaction failure
    match err {
        Error::TransactionFailed(_) => { /* Expected */ }
        Error::Rpc(_) => { /* RPC errors also acceptable */ }
        _ => panic!("Expected TransactionFailed, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_typed_contract_view_returns_wrong_type() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Define a "wrong" contract interface that expects different return type
    #[near_kit::contract]
    trait WrongGuestbook {
        // This method returns u32 in the real contract, but we expect a String
        fn total_messages(&self) -> String;
    }

    let wrong_guestbook = near.contract::<dyn WrongGuestbook>(&contract_id);
    let result = wrong_guestbook.total_messages().await;

    // The result could succeed (if deserialization happens to work) or fail
    // depending on how the contract serializes the response
    match result {
        Ok(s) => {
            // Some numbers can deserialize as strings
            println!("Got unexpected success: {}", s);
        }
        Err(e) => {
            println!("Got expected error on type mismatch: {:?}", e);
            // JSON deserialization error expected
            assert!(matches!(e, Error::Json(_) | Error::Rpc(_)));
        }
    }
}

#[tokio::test]
async fn test_typed_contract_query_at_invalid_block() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Query at a non-existent block height
    let result: Result<u32, _> = guestbook.total_messages().at_block(999_999_999_u64).await;

    assert!(result.is_err(), "Should error for invalid block height");
    let err = result.unwrap_err();
    println!("Query at invalid block: {:?}", err);

    match err {
        Error::Rpc(RpcError::UnknownBlock(_)) => { /* Expected */ }
        Error::Rpc(_) => { /* Other RPC errors acceptable */ }
        _ => panic!("Expected RPC error, got: {:?}", err),
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[tokio::test]
async fn test_typed_contract_view_methods_still_work_without_signer() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client WITHOUT a signer
    let no_signer_near = Near::custom(sandbox.rpc_url()).build();
    let guestbook = no_signer_near.contract::<dyn Guestbook>(&contract_id);

    // View methods should still work without a signer
    let result = guestbook.total_messages().await;
    assert!(result.is_ok(), "View methods should work without signer");
    assert_eq!(result.unwrap(), 0);

    let messages = guestbook.get_messages().await;
    assert!(messages.is_ok(), "View methods should work without signer");
    assert!(messages.unwrap().is_empty());
}
