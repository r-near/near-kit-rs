//! Integration tests for signer edge cases.
//!
//! Tests various signer implementations and edge cases.
//! Run with: `cargo test --features sandbox --test integration signer_edge`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("signer{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// InMemorySigner Tests
// =============================================================================

#[tokio::test]
async fn test_in_memory_signer_from_secret_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create a new account
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client with InMemorySigner from secret key
    let near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Should be able to sign transactions
    let receiver_id: AccountId = format!("recv.{}", account_id).parse().unwrap();
    near.transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify the account was created
    let exists = near.account_exists(&receiver_id).await.unwrap();
    assert!(exists, "Receiver account should exist");
}

#[tokio::test]
async fn test_in_memory_signer_from_seed_phrase() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Generate a seed phrase (returns (phrase, key))
    let (phrase, key) = SecretKey::generate_with_seed_phrase().unwrap();
    let account_id = unique_account();

    // Create account with the key derived from seed phrase
    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Recreate the signer from the seed phrase (account_id first, then phrase)
    let signer = InMemorySigner::from_seed_phrase(&account_id, &phrase).unwrap();

    // Verify the public key matches (synchronous method)
    let pubkey = signer.public_key();
    assert_eq!(*pubkey, key.public_key(), "Public keys should match");

    // Create client with seed phrase signer
    let near = Near::custom(sandbox.rpc_url()).signer(signer).build();

    // Should be able to query account
    let balance = near.balance(&account_id).await.unwrap();
    println!("Balance for seed-phrase account: {}", balance);
    assert!(balance.total.as_near() > 40, "Should have NEAR balance");
}

// =============================================================================
// RotatingSigner Tests
// =============================================================================

#[tokio::test]
async fn test_rotating_signer_uses_multiple_keys() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create an account with multiple access keys
    let key1 = SecretKey::generate_ed25519();
    let key2 = SecretKey::generate_ed25519();
    let key3 = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(key1.public_key())
        .add_full_access_key(key2.public_key())
        .add_full_access_key(key3.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a rotating signer with all three keys
    let signer = RotatingSigner::new(&account_id, vec![key1, key2, key3]).unwrap();

    let near = Near::custom(sandbox.rpc_url()).signer(signer).build();

    // Execute multiple transactions - the rotating signer should cycle through keys
    for i in 0..6 {
        let sub_id: AccountId = format!("sub{}.{}", i, account_id).parse().unwrap();
        near.transaction(&sub_id)
            .create_account()
            .transfer(NearToken::near(1))
            .add_full_access_key(SecretKey::generate_ed25519().public_key())
            .send()
            .wait_until(TxExecutionStatus::Final)
            .await
            .unwrap();
    }

    // All 6 accounts should exist
    for i in 0..6 {
        let sub_id: AccountId = format!("sub{}.{}", i, account_id).parse().unwrap();
        let exists = near.account_exists(&sub_id).await.unwrap();
        assert!(exists, "Sub-account {} should exist", i);
    }
}

#[tokio::test]
async fn test_rotating_signer_with_single_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Rotating signer with just one key should still work
    let signer = RotatingSigner::new(&account_id, vec![key]).unwrap();

    let near = Near::custom(sandbox.rpc_url()).signer(signer).build();

    // Should work fine with single key
    let sub_id: AccountId = format!("single.{}", account_id).parse().unwrap();
    near.transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let exists: bool = near.account_exists(&sub_id).await.unwrap();
    assert!(exists);
}

#[tokio::test]
async fn test_rotating_signer_empty_keys_fails() {
    let account_id = unique_account();

    // Empty keys should fail
    let result = RotatingSigner::new(&account_id, vec![]);

    assert!(result.is_err(), "Should fail with empty keys");
    match result.unwrap_err() {
        Error::Config(msg) => {
            assert!(
                msg.contains("at least one key"),
                "Error should mention keys: {}",
                msg
            );
        }
        e => panic!("Expected Config error, got: {:?}", e),
    }
}

// =============================================================================
// sign_with Override Tests
// =============================================================================

#[tokio::test]
async fn test_sign_with_override() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create two accounts with different keys
    let key1 = SecretKey::generate_ed25519();
    let key2 = SecretKey::generate_ed25519();
    let account1_id = unique_account();
    let account2_id = unique_account();

    root_near
        .transaction(&account1_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key1.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    root_near
        .transaction(&account2_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key2.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client with account1's signer
    let near = Near::custom(sandbox.rpc_url())
        .credentials(key1.to_string(), account1_id.as_str())
        .unwrap()
        .build();

    // Use sign_with to send a transaction from account2
    let signer2 = InMemorySigner::from_secret_key(account2_id.clone(), key2);

    let sub_id: AccountId = format!("signwith.{}", account2_id).parse().unwrap();
    near.transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .sign_with(signer2)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // The account should have been created by account2 (subaccount of account2)
    let exists: bool = near.account_exists(&sub_id).await.unwrap();
    assert!(exists);
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

#[tokio::test]
async fn test_wrong_key_for_account() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create an account with one key
    let correct_key = SecretKey::generate_ed25519();
    let wrong_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(correct_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client with the WRONG key
    let near = Near::custom(sandbox.rpc_url())
        .credentials(wrong_key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to send a transaction - should fail with access key error
    let sub_id: AccountId = format!("wrongkey.{}", account_id).parse().unwrap();
    let result = near
        .transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(result.is_err(), "Should fail with wrong key");
    let err = result.unwrap_err();
    println!("Wrong key error: {:?}", err);

    // Should be an access key not found or invalid transaction error
    match err {
        Error::Rpc(RpcError::AccessKeyNotFound { .. }) => { /* Expected */ }
        Error::Rpc(RpcError::InvalidTransaction { .. }) => { /* Also acceptable */ }
        Error::Rpc(_) => { /* Other RPC errors */ }
        _ => panic!("Expected RPC error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_deleted_key_fails() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create an account with two keys
    let key1 = SecretKey::generate_ed25519();
    let key2 = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(key1.public_key())
        .add_full_access_key(key2.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client with key1
    let near = Near::custom(sandbox.rpc_url())
        .credentials(key1.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Delete key1 using key2
    let near2 = Near::custom(sandbox.rpc_url())
        .credentials(key2.to_string(), account_id.as_str())
        .unwrap()
        .build();

    near2
        .delete_key(&account_id, key1.public_key())
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Now try to use the deleted key1
    let sub_id: AccountId = format!("deletedkey.{}", account_id).parse().unwrap();
    let result = near
        .transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(result.is_err(), "Should fail with deleted key");
    println!("Deleted key error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_signing_with_ed25519_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Test with Ed25519 key (most common)
    let ed_key = SecretKey::generate_ed25519();
    let ed_account = unique_account();

    root_near
        .transaction(&ed_account)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(ed_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let ed_near = Near::custom(sandbox.rpc_url())
        .credentials(ed_key.to_string(), ed_account.as_str())
        .unwrap()
        .build();

    // Should work with Ed25519
    let sub_id: AccountId = format!("ed25519.{}", ed_account).parse().unwrap();
    ed_near
        .transaction(&sub_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let exists: bool = ed_near.account_exists(&sub_id).await.unwrap();
    assert!(exists);
}
