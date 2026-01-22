//! Integration tests against NEAR testnet.
//!
//! These tests make real RPC calls to testnet. They are designed to be
//! read-only and not require any credentials.

use near_kit::prelude::*;

/// Test that we can query account balance on testnet.
#[tokio::test]
async fn test_balance_query() {
    let near = Near::testnet().build();

    // Query a well-known testnet account
    let balance = near.balance("near.testnet").await.unwrap();

    // The NEAR foundation account should have some balance
    assert!(balance.total.as_yoctonear() > 0);
    println!("near.testnet balance: {}", balance);
}

/// Test that we can query account info.
#[tokio::test]
async fn test_account_query() {
    let near = Near::testnet().build();

    let account = near.account("near.testnet").await.unwrap();

    assert!(account.storage_usage > 0);
    println!("near.testnet storage: {} bytes", account.storage_usage);
}

/// Test account existence check.
#[tokio::test]
async fn test_account_exists() {
    let near = Near::testnet().build();

    // This account should exist
    assert!(near.account_exists("near.testnet").await.unwrap());

    // This random account should not exist
    assert!(!near
        .account_exists("definitely-does-not-exist-12345.testnet")
        .await
        .unwrap());
}

/// Test access keys query.
#[tokio::test]
async fn test_access_keys_query() {
    let near = Near::testnet().build();

    let keys = near.access_keys("near.testnet").await.unwrap();

    // The account should have at least one access key
    assert!(!keys.keys.is_empty());
    println!("near.testnet has {} access keys", keys.keys.len());
}

/// Test view function call.
#[tokio::test]
async fn test_view_call() {
    let near = Near::testnet().build();

    // Call a view function on a well-known contract
    // The wrap.testnet contract has ft_balance_of
    let balance: String = near
        .view("wrap.testnet", "ft_balance_of")
        .args(serde_json::json!({ "account_id": "near.testnet" }))
        .await
        .unwrap();

    println!("wrap.testnet ft_balance_of(near.testnet) = {}", balance);
}

/// Test balance query at specific finality.
#[tokio::test]
async fn test_balance_with_finality() {
    let near = Near::testnet().build();

    // Query with optimistic finality (faster)
    let balance = near
        .balance("near.testnet")
        .finality(Finality::Optimistic)
        .await
        .unwrap();

    assert!(balance.total.as_yoctonear() > 0);
}

/// Test RPC status call.
#[tokio::test]
async fn test_rpc_status() {
    let near = Near::testnet().build();

    let status = near.rpc().status().await.unwrap();

    assert_eq!(status.chain_id, "testnet");
    println!(
        "Testnet block height: {}",
        status.sync_info.latest_block_height
    );
}

/// Test gas price query.
#[tokio::test]
async fn test_gas_price() {
    let near = Near::testnet().build();

    let gas_price = near.rpc().gas_price(None).await.unwrap();

    assert!(gas_price.as_u128() > 0);
    println!("Current gas price: {} yoctoNEAR", gas_price.gas_price);
}
