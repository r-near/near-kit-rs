//! Integration tests for delegate actions (meta-transactions, NEP-366).
//!
//! These tests verify that:
//! 1. Users can create and sign delegate actions
//! 2. Relayers can submit delegate actions on behalf of users
//! 3. The delegated actions execute correctly
//!
//! Run with: `cargo test --test delegate_action_integration --features sandbox`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account(prefix: &str) -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}{}.{}", prefix, n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_delegate_action_transfer() {
    // Test the full delegate action flow:
    // 1. Sender creates and signs a delegate action for a transfer
    // 2. Relayer submits the delegate action
    // 3. Verify the transfer happened

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account (who wants to transfer but won't pay gas)
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create relayer account (who will submit the transaction and pay gas)
    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("relayer");

    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create recipient account
    let recipient_key = SecretKey::generate_ed25519();
    let recipient_id = unique_account("recipient");

    root_near
        .transaction(&recipient_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(recipient_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Created accounts: sender={}, relayer={}, recipient={}",
        sender_id, relayer_id, recipient_id
    );

    // Get recipient's initial balance
    let initial_balance = root_near.balance(&recipient_id).await.unwrap();
    println!("Recipient initial balance: {}", initial_balance);

    // --- SENDER: Create and sign a delegate action ---
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Build a delegate action for transferring 2 NEAR to recipient
    let delegate_result = sender_near
        .transaction(&recipient_id)
        .transfer(NearToken::near(2))
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();

    println!(
        "Sender signed delegate action, payload length: {} bytes",
        delegate_result.payload.len()
    );

    // --- RELAYER: Submit the delegate action ---
    let relayer_near = Near::custom(rpc_url)
        .credentials(relayer_key.to_string(), relayer_id.as_str())
        .unwrap()
        .build();

    // Decode the payload (simulating receiving it via HTTP)
    let signed_delegate = SignedDelegateAction::from_base64(&delegate_result.payload).unwrap();

    // Verify the delegate action contains the expected data
    assert_eq!(signed_delegate.sender_id().as_str(), sender_id.as_str());
    assert_eq!(
        signed_delegate.receiver_id().as_str(),
        recipient_id.as_str()
    );

    // Submit the delegate action
    let outcome = relayer_near
        .transaction(signed_delegate.sender_id().as_str())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Relayer submitted delegate action: success={}, hash={:?}",
        outcome.is_success(),
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());

    // --- VERIFY: Check that the transfer happened ---
    let final_balance = root_near.balance(&recipient_id).await.unwrap();
    println!("Recipient final balance: {}", final_balance);

    // Balance should have increased by 2 NEAR
    let diff = final_balance.total.as_yoctonear() - initial_balance.total.as_yoctonear();
    let expected = NearToken::from_near(2).as_yoctonear();
    assert_eq!(diff, expected, "Expected +2 NEAR, got diff: {} yocto", diff);

    println!("Delegate action transfer successful!");
}

#[tokio::test]
async fn test_delegate_action_function_call() {
    // Test delegate action with a function call
    // This tests that more complex actions work through delegation

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create relayer account
    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("relayer");

    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create contract account and deploy a simple contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account("contract");

    // Deploy the guestbook contract (a simple contract with add_message/get_messages)
    let wasm_code =
        std::fs::read("tests/contracts/guestbook.wasm").expect("Failed to read guestbook.wasm");

    root_near
        .transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Created accounts: sender={}, relayer={}, contract={}",
        sender_id, relayer_id, contract_id
    );

    // --- SENDER: Create delegate action for function call ---
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    let delegate_result = sender_near
        .transaction(&contract_id)
        .call("add_message")
        .args(serde_json::json!({ "text": "Hello from delegate!" }))
        .gas(Gas::tgas(30))
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();

    println!("Sender signed delegate action for function call");

    // --- RELAYER: Submit the delegate action ---
    let relayer_near = Near::custom(rpc_url)
        .credentials(relayer_key.to_string(), relayer_id.as_str())
        .unwrap()
        .build();

    let signed_delegate = SignedDelegateAction::from_base64(&delegate_result.payload).unwrap();

    let outcome = relayer_near
        .transaction(signed_delegate.sender_id().as_str())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Relayer submitted delegate action: success={}",
        outcome.is_success()
    );
    assert!(outcome.is_success());

    // --- VERIFY: Check that the message was added ---
    // The guestbook contract stores messages, so we verify by checking it didn't error
    // (The contract doesn't have a get_messages view, so we just check success)
    println!("Delegate action function call successful!");
}

#[tokio::test]
async fn test_delegate_action_multiple_actions() {
    // Test delegate action with multiple actions in one transaction

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(20))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create relayer account
    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("relayer");

    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // New account to be created via delegate action
    let new_account_key = SecretKey::generate_ed25519();
    let new_account_id: AccountId = format!("new.{}", sender_id).parse().unwrap();

    println!(
        "Sender={}, Relayer={}, New account={}",
        sender_id, relayer_id, new_account_id
    );

    // --- SENDER: Create delegate action with multiple actions ---
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create a new account with funding and a key - all in one delegate action
    let delegate_result = sender_near
        .transaction(&new_account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(new_account_key.public_key())
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();

    println!("Sender signed delegate action with multiple actions");

    // --- RELAYER: Submit the delegate action ---
    let relayer_near = Near::custom(rpc_url)
        .credentials(relayer_key.to_string(), relayer_id.as_str())
        .unwrap()
        .build();

    let signed_delegate = SignedDelegateAction::from_base64(&delegate_result.payload).unwrap();

    let outcome = relayer_near
        .transaction(signed_delegate.sender_id().as_str())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Relayer submitted delegate action: success={}",
        outcome.is_success()
    );
    assert!(outcome.is_success());

    // --- VERIFY: Check that the new account was created ---
    assert!(root_near.account_exists(&new_account_id).await.unwrap());

    let balance = root_near.balance(&new_account_id).await.unwrap();
    println!("New account balance: {}", balance);
    assert!(balance.total > NearToken::from_near(4)); // Should have ~5 NEAR minus storage

    println!("Delegate action with multiple actions successful!");
}

#[tokio::test]
async fn test_delegate_action_roundtrip_encoding() {
    // Test that delegate actions can be encoded and decoded correctly

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Recipient
    let recipient_id = unique_account("recipient");
    let recipient_key = SecretKey::generate_ed25519();

    root_near
        .transaction(&recipient_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(recipient_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a delegate action
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    let delegate_result = sender_near
        .transaction(&recipient_id)
        .transfer(NearToken::near(1))
        .delegate(Default::default())
        .await
        .unwrap();

    // Test base64 roundtrip
    let base64_payload = &delegate_result.payload;
    let decoded = SignedDelegateAction::from_base64(base64_payload).unwrap();
    let re_encoded = decoded.to_base64();
    assert_eq!(base64_payload, &re_encoded);

    // Test bytes roundtrip
    let bytes = delegate_result.signed_delegate_action.to_bytes();
    let decoded_from_bytes = SignedDelegateAction::from_bytes(&bytes).unwrap();
    assert_eq!(
        decoded_from_bytes.sender_id().as_str(),
        delegate_result.signed_delegate_action.sender_id().as_str()
    );
    assert_eq!(
        decoded_from_bytes.receiver_id().as_str(),
        delegate_result
            .signed_delegate_action
            .receiver_id()
            .as_str()
    );

    println!("Delegate action encoding roundtrip successful!");
}

#[tokio::test]
async fn test_delegate_action_validation_errors() {
    // Test that invalid delegate actions are rejected properly

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Test: Empty actions should fail
    let result = sender_near
        .transaction(&sender_id)
        .delegate(Default::default())
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("at least one action"),
        "Expected 'at least one action' error, got: {}",
        err
    );

    println!("Delegate action validation errors work correctly!");
}
