//! Integration tests running against near-sandbox.
//!
//! These tests use a shared sandbox instance with unique subaccounts per test,
//! following the pattern from defuse-sandbox.
//!
//! Run with: `cargo test --test sandbox_integration --features sandbox`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("test{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_sandbox_balance() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create a test account
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(1000))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let balance = root_near.balance(&account_id).await.unwrap();
    println!("Test account balance: {}", balance);

    // Should have approximately 1000 NEAR (minus account creation costs)
    assert!(balance.total > NearToken::from_near(999));
}

#[tokio::test]
async fn test_sandbox_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create sender's client
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Generate a new keypair for the receiver
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", sender_id).parse().unwrap();

    // Create the receiver account
    let outcome = sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Create account outcome: success={}, hash={:?}",
        outcome.is_success(),
        outcome.transaction_hash()
    );

    // Check the receiver's balance
    let balance = root_near.balance(&receiver_id).await.unwrap();
    println!("Receiver balance: {}", balance);

    // Should have approximately 10 NEAR (minus storage costs)
    assert!(balance.total > NearToken::from_near(9));
    assert!(balance.total < NearToken::from_near(11));
}

#[tokio::test]
async fn test_sandbox_multiple_transfers() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Generate keypairs for multiple receivers
    let receiver1_key = SecretKey::generate_ed25519();
    let receiver1_id: AccountId = format!("receiver1.{}", sender_id).parse().unwrap();

    let receiver2_key = SecretKey::generate_ed25519();
    let receiver2_id: AccountId = format!("receiver2.{}", sender_id).parse().unwrap();

    // Create first account
    sender_near
        .transaction(&receiver1_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(receiver1_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create second account
    sender_near
        .transaction(&receiver2_id)
        .create_account()
        .transfer(NearToken::near(3))
        .add_full_access_key(receiver2_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check balances
    let balance1 = root_near.balance(&receiver1_id).await.unwrap();
    let balance2 = root_near.balance(&receiver2_id).await.unwrap();

    println!("Receiver1 balance: {}", balance1);
    println!("Receiver2 balance: {}", balance2);

    assert!(balance1.total > NearToken::from_near(4));
    assert!(balance2.total > NearToken::from_near(2));
}

#[tokio::test]
async fn test_sandbox_simple_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create a receiver account
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("bob.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let initial_balance = root_near.balance(&receiver_id).await.unwrap();

    // Now do a simple transfer using the convenience method
    sender_near
        .transfer(&receiver_id, NearToken::near(2))
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let final_balance = root_near.balance(&receiver_id).await.unwrap();

    println!("Initial: {}, Final: {}", initial_balance, final_balance);

    // Balance should have increased by ~2 NEAR
    let diff = final_balance.total.as_yoctonear() - initial_balance.total.as_yoctonear();
    let expected = NearToken::from_near(2).as_yoctonear();
    assert!(diff == expected, "Expected +2 NEAR, got diff: {}", diff);
}

#[tokio::test]
async fn test_sandbox_create_account_outcome() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create a contract account
    let contract_key = SecretKey::generate_ed25519();
    let contract_id: AccountId = format!("contract.{}", sender_id).parse().unwrap();

    // Create account with funding
    let outcome = sender_near
        .transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(contract_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("Transaction hash: {:?}", outcome.transaction_hash());
    println!("Gas used: {}", outcome.total_gas_used());

    assert!(outcome.is_success());
}

#[tokio::test]
async fn test_sandbox_delete_account() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create parent account
    let parent_key = SecretKey::generate_ed25519();
    let parent_id = unique_account();

    root_near
        .transaction(&parent_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(parent_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let parent_near = Near::custom(rpc_url)
        .credentials(parent_key.to_string(), parent_id.as_str())
        .unwrap()
        .build();

    // Create an account to delete
    let temp_key = SecretKey::generate_ed25519();
    let temp_id: AccountId = format!("temporary.{}", parent_id).parse().unwrap();

    parent_near
        .transaction(&temp_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(temp_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify it exists
    assert!(root_near.account_exists(&temp_id).await.unwrap());

    // Create a new client with the temp account's key to delete it
    let temp_near = Near::custom(rpc_url)
        .credentials(temp_key.to_string(), temp_id.as_str())
        .unwrap()
        .build();

    // Delete the account, sending remaining balance to parent account
    temp_near
        .transaction(&temp_id)
        .delete_account(&parent_id)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify it no longer exists
    assert!(!root_near.account_exists(&temp_id).await.unwrap());
}

#[tokio::test]
async fn test_sandbox_add_and_delete_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create an account
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a new client for this account
    let account_near = Near::custom(rpc_url)
        .credentials(account_key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Add a second key
    let second_key = SecretKey::generate_ed25519();

    account_near
        .transaction(&account_id)
        .add_full_access_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check that both keys exist
    let keys = account_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 2);

    // Delete the second key
    account_near
        .transaction(&account_id)
        .delete_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check that only one key remains
    let keys = account_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 1);
}

#[tokio::test]
async fn test_sandbox_multiple_actions_in_one_transaction() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create parent account
    let parent_key = SecretKey::generate_ed25519();
    let parent_id = unique_account();

    root_near
        .transaction(&parent_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(parent_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let parent_near = Near::custom(rpc_url)
        .credentials(parent_key.to_string(), parent_id.as_str())
        .unwrap()
        .build();

    // Create two accounts in separate transactions
    let alice_key = SecretKey::generate_ed25519();
    let alice_id: AccountId = format!("alice.{}", parent_id).parse().unwrap();

    let bob_key = SecretKey::generate_ed25519();
    let bob_id: AccountId = format!("bob.{}", parent_id).parse().unwrap();

    // Create alice
    parent_near
        .transaction(&alice_id)
        .create_account()
        .transfer(NearToken::near(20))
        .add_full_access_key(alice_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create bob
    parent_near
        .transaction(&bob_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(bob_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify both exist
    assert!(root_near.account_exists(&alice_id).await.unwrap());
    assert!(root_near.account_exists(&bob_id).await.unwrap());

    let alice_balance = root_near.balance(&alice_id).await.unwrap();
    let bob_balance = root_near.balance(&bob_id).await.unwrap();

    println!("Alice: {}, Bob: {}", alice_balance, bob_balance);

    assert!(alice_balance.total > NearToken::from_near(19));
    assert!(bob_balance.total > NearToken::from_near(9));
}
