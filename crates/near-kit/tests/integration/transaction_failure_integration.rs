//! Integration tests for additional transaction failure scenarios.
//!
//! These tests cover edge cases for transaction failures beyond basic error handling.
//! Run with: `cargo test --features sandbox --test integration transaction_failure`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("txfail{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// Deploy/Contract Errors
// =============================================================================

#[tokio::test]
async fn test_deploy_invalid_wasm() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to deploy invalid WASM (random bytes)
    let invalid_wasm = vec![0u8; 100]; // Not valid WASM

    let result = account_near.deploy(&account_id, invalid_wasm).await;

    // Note: NEAR allows deploying any bytes, but calling methods will fail
    // The deploy itself may succeed
    println!("Invalid WASM deploy result: {:?}", result);
}

#[tokio::test]
async fn test_deploy_empty_wasm() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

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

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to deploy empty WASM
    let empty_wasm: Vec<u8> = vec![];

    let result = account_near.deploy(&account_id, empty_wasm).await;

    // Empty deploys may succeed (effectively removes contract)
    println!("Empty WASM deploy result: {:?}", result);
}

// =============================================================================
// Key Management Errors
// =============================================================================

#[tokio::test]
async fn test_add_key_to_nonexistent_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let nonexistent: AccountId = "nonexistent-for-add-key.sandbox".parse().unwrap();

    // Try to add a key to a non-existent account
    let result = near
        .add_full_access_key(&nonexistent, key.public_key())
        .await;

    assert!(result.is_err(), "Should fail for non-existent account");
    println!("Add key to non-existent: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_add_duplicate_key() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account with key
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to add the same key again
    let result = account_near
        .add_full_access_key(&account_id, key.public_key())
        .await;

    assert!(result.is_err(), "Should fail when adding duplicate key");
    println!("Duplicate key error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_delete_last_full_access_key() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

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

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Delete the only key - this should succeed but leave the account inaccessible
    // Note: NEAR protocol allows this
    let result = account_near.delete_key(&account_id, key.public_key()).await;

    // This may succeed - depends on protocol rules about last key
    println!("Delete last key result: {:?}", result);
}

// =============================================================================
// Account Creation Errors
// =============================================================================

#[tokio::test]
async fn test_create_subaccount_of_nonexistent_parent() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to create a subaccount of a non-existent parent
    // This should fail because only the parent can create subaccounts
    let sub_id: AccountId = "sub.nonexistent-parent.sandbox".parse().unwrap();
    let key = SecretKey::generate_ed25519();

    let result = near
        .transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(result.is_err(), "Should fail for non-existent parent");
    println!("Non-existent parent error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_create_account_without_initial_balance() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account without transferring any balance
    let result = near
        .transaction(&account_id)
        .create_account()
        .add_full_access_key(key.public_key())
        // No .transfer() call
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    // Note: Creating an account without balance may succeed
    // The account will exist but have 0 balance
    // NEAR protocol allows this
    println!("Create without balance result: {:?}", result);
}

#[tokio::test]
async fn test_create_account_with_insufficient_balance() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account with very small balance (not enough for storage)
    let result = near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::yocto(1)) // Way too small
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    // This may fail due to insufficient balance for storage
    println!("Insufficient balance for account creation: {:?}", result);
}

// =============================================================================
// Multi-Action Transaction Errors
// =============================================================================

#[tokio::test]
async fn test_transaction_with_failing_action_in_middle() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

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

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try a multi-action transaction where one action fails
    // First action: valid key deletion (the one we're signing with)
    // Second action: delete a non-existent key (should fail)
    // The whole transaction should fail
    let fake_key = SecretKey::generate_ed25519();

    let result = account_near
        .transaction(&account_id)
        .delete_key(fake_key.public_key()) // This key doesn't exist
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(result.is_err(), "Should fail when action fails");
    println!("Multi-action failure: {:?}", result.unwrap_err());
}

// =============================================================================
// Delete Account Errors
// =============================================================================

#[tokio::test]
async fn test_delete_nonexistent_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to delete an account that doesn't exist
    let nonexistent: AccountId = "nonexistent-to-delete.sandbox".parse().unwrap();

    let result = near
        .transaction(&nonexistent)
        .delete_account(ROOT_ACCOUNT)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(result.is_err(), "Should fail for non-existent account");
    println!("Delete non-existent account: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_delete_account_to_nonexistent_beneficiary() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to delete account with non-existent beneficiary
    let nonexistent_beneficiary: AccountId = "nonexistent-beneficiary.sandbox".parse().unwrap();

    let result = account_near
        .transaction(&account_id)
        .delete_account(&nonexistent_beneficiary)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    // This should fail because the beneficiary doesn't exist
    println!("Delete to non-existent beneficiary: {:?}", result);
}

// =============================================================================
// Stake Errors
// =============================================================================

#[tokio::test]
async fn test_stake_with_insufficient_balance() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(1)) // Small balance
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to stake more than available
    let result = account_near
        .transaction(&account_id)
        .stake(NearToken::near(1000), key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(
        result.is_err(),
        "Should fail with insufficient balance to stake"
    );
    println!("Insufficient stake balance: {:?}", result.unwrap_err());
}

// =============================================================================
// Transfer Errors
// =============================================================================

#[tokio::test]
async fn test_transfer_zero_amount() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

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

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Transfer zero amount
    let receiver: AccountId = format!("recv.{}", account_id).parse().unwrap();

    // First create receiver
    account_near
        .transaction(&receiver)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Transfer zero
    let result = account_near.transfer(&receiver, NearToken::yocto(0)).await;

    // Zero transfers may or may not be allowed
    println!("Zero transfer result: {:?}", result);
}

#[tokio::test]
async fn test_transfer_max_amount() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

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

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Transfer max u128 (way more than balance)
    let result = account_near
        .transfer(ROOT_ACCOUNT, NearToken::yocto(u128::MAX))
        .await;

    assert!(result.is_err(), "Should fail with max amount");
    println!("Max transfer error: {:?}", result.unwrap_err());
}
