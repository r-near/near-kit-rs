//! Integration tests for typed contract interfaces.
//!
//! Tests the `#[near_kit::contract]` macro with the guestbook contract.

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, SandboxNetwork};
use serde::{Deserialize, Serialize};

// ============================================================================
// Define the typed contract interface for guestbook
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
// Helper to deploy guestbook contract
// ============================================================================

async fn deploy_guestbook(near: &Near, contract_account: &str) -> Result<(), near_kit::Error> {
    let wasm_code = std::fs::read("tests/contracts/guestbook.wasm")
        .expect("guestbook.wasm not found in tests/contracts/");

    let new_key = SecretKey::generate_ed25519();

    near.transaction(contract_account)
        .create_account()
        .transfer("10 NEAR")
        .add_full_access_key(new_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_typed_contract_view_methods() {
    // Start sandbox
    let sandbox = SandboxConfig::fresh().await;
    let near = Near::sandbox(&sandbox);

    // Deploy guestbook contract
    let contract_id = format!("guestbook.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id)
        .await
        .expect("Failed to deploy guestbook");

    // Create typed contract client
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Test view method - total_messages
    let count = guestbook
        .total_messages()
        .await
        .expect("Failed to get total_messages");
    assert_eq!(count, 0, "Initial message count should be 0");

    // Test view method - get_messages
    let messages = guestbook
        .get_messages()
        .await
        .expect("Failed to get messages");
    assert!(messages.is_empty(), "Initial messages should be empty");

    println!("✓ Typed contract view methods work correctly");
}

#[tokio::test]
async fn test_typed_contract_call_methods() {
    // Start sandbox
    let sandbox = SandboxConfig::fresh().await;
    let near = Near::sandbox(&sandbox);

    // Deploy guestbook contract
    let contract_id = format!("guestbook.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id)
        .await
        .expect("Failed to deploy guestbook");

    // Create typed contract client
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Add a message using typed call method
    guestbook
        .add_message(AddMessageArgs {
            text: "Hello from typed contract!".to_string(),
        })
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect("Failed to add message");

    // Verify the message was added
    let count = guestbook
        .total_messages()
        .await
        .expect("Failed to get total_messages");
    assert_eq!(count, 1, "Message count should be 1 after adding");

    let messages = guestbook
        .get_messages()
        .await
        .expect("Failed to get messages");

    assert_eq!(messages.len(), 1, "Should have exactly 1 message");
    assert_eq!(messages[0].text, "Hello from typed contract!");
    assert_eq!(messages[0].sender, sandbox.root_account_id());

    println!("✓ Typed contract call methods work correctly");
}

#[tokio::test]
async fn test_typed_contract_multiple_messages() {
    // Start sandbox
    let sandbox = SandboxConfig::fresh().await;
    let near = Near::sandbox(&sandbox);

    // Deploy guestbook contract
    let contract_id = format!("guestbook.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id)
        .await
        .expect("Failed to deploy guestbook");

    // Create typed contract client
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Add multiple messages
    let test_messages = vec!["First message", "Second message", "Third message"];

    for text in &test_messages {
        guestbook
            .add_message(AddMessageArgs {
                text: text.to_string(),
            })
            .wait_until(TxExecutionStatus::Final)
            .await
            .expect("Failed to add message");
    }

    // Verify count
    let count = guestbook
        .total_messages()
        .await
        .expect("Failed to get total_messages");
    assert_eq!(count, 3, "Should have 3 messages");

    // Verify messages
    let messages = guestbook
        .get_messages()
        .await
        .expect("Failed to get messages");

    assert_eq!(messages.len(), 3);
    for (i, msg) in messages.iter().enumerate() {
        assert_eq!(msg.text, test_messages[i]);
    }

    println!("✓ Multiple messages work correctly");
}

#[tokio::test]
async fn test_typed_contract_with_custom_gas() {
    // Start sandbox
    let sandbox = SandboxConfig::fresh().await;
    let near = Near::sandbox(&sandbox);

    // Deploy guestbook contract
    let contract_id = format!("guestbook.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id)
        .await
        .expect("Failed to deploy guestbook");

    // Create typed contract client
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Add message with custom gas
    guestbook
        .add_message(AddMessageArgs {
            text: "Message with custom gas".to_string(),
        })
        .gas("50 Tgas")
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect("Failed to add message with custom gas");

    // Verify
    let count = guestbook
        .total_messages()
        .await
        .expect("Failed to get total_messages");
    assert_eq!(count, 1);

    println!("✓ Custom gas works correctly");
}

#[tokio::test]
async fn test_typed_contract_block_reference() {
    // Start sandbox
    let sandbox = SandboxConfig::fresh().await;
    let near = Near::sandbox(&sandbox);

    // Deploy guestbook contract
    let contract_id = format!("guestbook.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id)
        .await
        .expect("Failed to deploy guestbook");

    // Create typed contract client
    let guestbook = near.contract::<dyn Guestbook>(&contract_id);

    // Add a message
    guestbook
        .add_message(AddMessageArgs {
            text: "Test message".to_string(),
        })
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect("Failed to add message");

    // Query with optimistic finality
    let count = guestbook
        .total_messages()
        .finality(Finality::Optimistic)
        .await
        .expect("Failed to get total_messages with optimistic finality");

    assert_eq!(count, 1);

    println!("✓ Block reference on view methods works correctly");
}
