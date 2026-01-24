//! Integration tests for token helper error paths.
//!
//! These tests verify that FT and NFT helpers handle errors correctly.
//! Run with: `cargo test --features sandbox --test integration token_error`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tokerr{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// FT Error Cases
// =============================================================================

#[tokio::test]
async fn test_ft_metadata_on_non_contract_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create an account without any contract deployed
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

    // Try to get FT metadata from non-contract account
    let ft = near.ft(&account_id).unwrap();
    let result = ft.metadata().await;

    assert!(result.is_err(), "Should error for account without contract");
    let err = result.unwrap_err();

    // Should be ContractNotDeployed or similar error
    println!("FT metadata on non-contract: {:?}", err);
    match err {
        Error::Rpc(RpcError::ContractNotDeployed(_)) => {
            // Expected
        }
        _ => {
            // Accept other RPC errors that indicate no contract
            assert!(matches!(err, Error::Rpc(_)));
        }
    }
}

#[tokio::test]
async fn test_ft_metadata_on_nonexistent_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to get FT metadata from non-existent account
    let ft = near.ft("nonexistent-ft-contract.sandbox").unwrap();
    let result = ft.metadata().await;

    assert!(result.is_err(), "Should error for non-existent account");
    println!("FT metadata on non-existent: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_ft_balance_of_on_non_contract() {
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

    // Try to get balance from non-contract account
    let ft = near.ft(&account_id).unwrap();
    let result = ft.balance_of("alice.near").await;

    assert!(
        result.is_err(),
        "Should error for account without FT contract"
    );
    println!("FT balance_of on non-contract: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_ft_transfer_without_signer() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy a real FT contract
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    near.transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a client WITHOUT a signer
    let no_signer_near = Near::custom(sandbox.rpc_url()).build();

    // Try to transfer without a signer configured
    let ft = no_signer_near.ft(&owner_id).unwrap();
    let result = ft.transfer("bob.near", 100_u128).await;

    assert!(result.is_err(), "Should error when no signer configured");
    let err = result.unwrap_err();
    println!("FT transfer without signer: {:?}", err);

    match err {
        Error::NoSigner => {
            // Expected
        }
        _ => panic!("Expected NoSigner, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_ft_storage_deposit_without_signer() {
    let sandbox = SandboxConfig::shared().await;

    // Create a client WITHOUT a signer
    let no_signer_near = Near::custom(sandbox.rpc_url()).build();

    let ft = no_signer_near.ft("any-token.sandbox").unwrap();
    let result = ft.storage_deposit("alice.near").await;

    assert!(result.is_err(), "Should error when no signer configured");
    match result.unwrap_err() {
        Error::NoSigner => { /* Expected */ }
        e => panic!("Expected NoSigner, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_ft_on_wrong_contract_type() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy the guestbook contract (not an FT)
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

    // Try to use FT methods on non-FT contract (no init needed)
    let ft = near.ft(&contract_id).unwrap();
    let result = ft.metadata().await;

    assert!(result.is_err(), "Should error for non-FT contract");
    let err = result.unwrap_err();
    println!("FT metadata on guestbook: {:?}", err);

    // Could be ContractExecution or other RPC error
    match err {
        Error::Rpc(RpcError::ContractExecution { .. }) => { /* Expected */ }
        Error::Rpc(_) => { /* Other RPC errors are acceptable too */ }
        _ => panic!("Expected RPC error, got: {:?}", err),
    }
}

// =============================================================================
// NFT Error Cases
// =============================================================================

#[tokio::test]
async fn test_nft_metadata_on_non_contract() {
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

    let nft = near.nft(&account_id).unwrap();
    let result = nft.metadata().await;

    assert!(result.is_err(), "Should error for account without contract");
    println!("NFT metadata on non-contract: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_nft_token_on_non_contract() {
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

    let nft = near.nft(&account_id).unwrap();
    let result = nft.token("any-token").await;

    assert!(result.is_err(), "Should error for account without contract");
    println!("NFT token on non-contract: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_nft_transfer_without_signer() {
    let sandbox = SandboxConfig::shared().await;

    let no_signer_near = Near::custom(sandbox.rpc_url()).build();

    let nft = no_signer_near.nft("any-nft.sandbox").unwrap();
    let result = nft.transfer("bob.near", "token-1").await;

    assert!(result.is_err(), "Should error when no signer configured");
    match result.unwrap_err() {
        Error::NoSigner => { /* Expected */ }
        e => panic!("Expected NoSigner, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_nft_on_wrong_contract_type() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook (not an NFT)
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

    // Try to use NFT methods on non-NFT contract (no init needed)
    let nft = near.nft(&contract_id).unwrap();
    let result = nft.metadata().await;

    assert!(result.is_err(), "Should error for non-NFT contract");
    println!("NFT metadata on guestbook: {:?}", result.unwrap_err());
}

// =============================================================================
// Invalid Account ID Tests
// =============================================================================

#[tokio::test]
async fn test_ft_with_invalid_account_id() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to create FT client with invalid account ID
    let result = near.ft("INVALID-UPPERCASE.near");

    // Should fail during AccountId parsing
    assert!(result.is_err(), "Should error for invalid account ID");
    println!("FT with invalid account: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_nft_with_invalid_account_id() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let result = near.nft("has spaces.near");

    assert!(result.is_err(), "Should error for invalid account ID");
    println!("NFT with invalid account: {:?}", result.unwrap_err());
}
