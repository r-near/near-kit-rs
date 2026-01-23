#![cfg(feature = "sandbox")]
//! Basic integration tests against the sandbox.
//!
//! These tests verify core Near client functionality using a local sandbox.
//! Run with: `cargo test --test basic_integration --features sandbox`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("basic{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

/// Test that we can query account balance.
#[tokio::test]
async fn test_balance_query() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Query the root account
    let balance = near.balance(ROOT_ACCOUNT).await.unwrap();

    // The sandbox root account should have a large balance
    assert!(balance.total.as_yoctonear() > 0);
    println!("{} balance: {}", ROOT_ACCOUNT, balance);
}

/// Test that we can query account info.
#[tokio::test]
async fn test_account_query() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let account = near.account(ROOT_ACCOUNT).await.unwrap();

    assert!(account.storage_usage > 0);
    println!("{} storage: {} bytes", ROOT_ACCOUNT, account.storage_usage);
}

/// Test account existence check.
#[tokio::test]
async fn test_account_exists() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Root account should exist
    assert!(near.account_exists(ROOT_ACCOUNT).await.unwrap());

    // This random account should not exist
    assert!(!near
        .account_exists("definitely-does-not-exist-12345.sandbox")
        .await
        .unwrap());
}

/// Test access keys query.
#[tokio::test]
async fn test_access_keys_query() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let keys = near.access_keys(ROOT_ACCOUNT).await.unwrap();

    // The root account should have at least one access key
    assert!(!keys.keys.is_empty());
    println!("{} has {} access keys", ROOT_ACCOUNT, keys.keys.len());
}

/// Test balance query at specific finality.
#[tokio::test]
async fn test_balance_with_finality() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Query with optimistic finality
    let balance = near
        .balance(ROOT_ACCOUNT)
        .finality(Finality::Optimistic)
        .await
        .unwrap();

    assert!(balance.total.as_yoctonear() > 0);
}

/// Test RPC status call.
#[tokio::test]
async fn test_rpc_status() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let status = near.rpc().status().await.unwrap();

    // Sandbox uses "localnet" as chain_id
    assert!(!status.chain_id.is_empty());
    println!(
        "Sandbox chain_id: {}, block height: {}",
        status.chain_id, status.sync_info.latest_block_height
    );
}

/// Test gas price query.
#[tokio::test]
async fn test_gas_price() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let gas_price = near.rpc().gas_price(None).await.unwrap();

    assert!(gas_price.as_u128() > 0);
    println!("Current gas price: {} yoctoNEAR", gas_price.gas_price);
}

/// Test creating an account and querying it.
#[tokio::test]
async fn test_create_and_query_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create a new account
    let new_account_key = SecretKey::generate_ed25519();
    let new_account_id = unique_account();

    near.transaction(&new_account_id)
        .create_account()
        .transfer("10 NEAR")
        .add_full_access_key(new_account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Query the new account
    let account = near.account(&new_account_id).await.unwrap();
    assert!(account.amount.as_near() >= 9); // ~10 NEAR minus storage costs

    // Check it exists
    assert!(near.account_exists(&new_account_id).await.unwrap());

    // Check access keys
    let keys = near.access_keys(&new_account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 1);
}
