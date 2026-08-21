//! Integration tests for delegate actions (meta-transactions, NEP-366).
//!
//! These tests verify that:
//! 1. Users can create and sign delegate actions
//! 2. Relayers can submit delegate actions on behalf of users
//! 3. The delegated actions execute correctly
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::protocol::SignedDelegateAction;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::{DelegateOptions, Final};
use near_kit::*;

use super::support::{guestbook_wasm, unique_account};

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_delegate_action_transfer() {
    // Test the full delegate action flow:
    // 1. Sender creates and signs a delegate action for a transfer
    // 2. Relayer submits the delegate action
    // 3. Verify the transfer happened

    let (sandbox, root_near) = super::support::shared_client().await;

    // Create sender account (who wants to transfer but won't pay gas)
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    // Create relayer account (who will submit the transaction and pay gas)
    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("relayer");

    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    // Create recipient account
    let recipient_key = SecretKey::generate_ed25519();
    let recipient_id = unique_account("recipient");

    root_near
        .transaction(&recipient_id)
        .create_account()
        .transfer(NearToken::from_near(1))
        .add_full_access_key(recipient_key.public_key())
        .send()
        .wait_until::<Final>()
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
    let sender_near = Near::sandbox(sandbox)
        .with_signer(InMemorySigner::new(&sender_id, sender_key.to_string()).unwrap());

    // Build a delegate action for transferring 2 NEAR to recipient
    let delegate_result = sender_near
        .transaction(&recipient_id)
        .transfer(NearToken::from_near(2))
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();

    println!(
        "Sender signed delegate action, payload length: {} bytes",
        delegate_result.payload.len()
    );

    // --- RELAYER: Submit the delegate action ---
    let relayer_near = Near::sandbox(sandbox)
        .with_signer(InMemorySigner::new(&relayer_id, relayer_key.to_string()).unwrap());

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
        .transaction(signed_delegate.sender_id())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    println!(
        "Relayer submitted delegate action: hash={:?}",
        outcome.transaction_hash()
    );

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

    let (sandbox, root_near) = super::support::shared_client().await;

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("sender");

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    // Create relayer account
    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("relayer");

    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    // Create contract account and deploy a simple contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account("contract");

    // Deploy the guestbook contract (a simple contract with add_message/get_messages)
    let wasm_code = guestbook_wasm();

    root_near
        .transaction(&contract_id)
        .create_account()
        .transfer(NearToken::from_near(5))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    println!(
        "Created accounts: sender={}, relayer={}, contract={}",
        sender_id, relayer_id, contract_id
    );

    // --- SENDER: Create delegate action for function call ---
    let sender_near = Near::sandbox(sandbox)
        .with_signer(InMemorySigner::new(&sender_id, sender_key.to_string()).unwrap());

    let delegate_result = sender_near
        .transaction(&contract_id)
        .call("add_message")
        .args(serde_json::json!({ "text": "Hello from delegate!" }))
        .gas(Gas::from_tgas(30))
        .finish()
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();

    println!("Sender signed delegate action for function call");

    // --- RELAYER: Submit the delegate action ---
    let relayer_near = Near::sandbox(sandbox)
        .with_signer(InMemorySigner::new(&relayer_id, relayer_key.to_string()).unwrap());

    let signed_delegate = SignedDelegateAction::from_base64(&delegate_result.payload).unwrap();

    let _outcome = relayer_near
        .transaction(signed_delegate.sender_id())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    println!("Delegate action function call successful!");
}
