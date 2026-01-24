//! Integration tests for token helpers (FT and NFT).
//!
//! Run with: `cargo test --features sandbox --test integration token_integration`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("token{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// Fungible Token Tests
// =============================================================================

/// Deploy a fungible token contract and return the contract account ID
async fn deploy_ft_contract(
    near: &Near,
    owner_id: &AccountId,
) -> Result<(AccountId, SecretKey), Error> {
    let ft_key = SecretKey::generate_ed25519();
    let ft_id = unique_account();

    let wasm = std::fs::read("tests/contracts/fungible_token.wasm")
        .expect("fungible_token.wasm not found");

    // Create FT contract account with wasm deployed
    near.transaction(&ft_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(ft_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await?;

    // Initialize the FT contract
    // The near-sdk-rs example FT uses "new" with owner_id, total_supply, metadata
    let ft_near = Near::custom(near.rpc_url())
        .credentials(ft_key.to_string(), ft_id.as_str())?
        .build();

    ft_near
        .call(&ft_id, "new")
        .args(serde_json::json!({
            "owner_id": owner_id.to_string(),
            "total_supply": "1000000000000000000000000",  // 1M tokens with 18 decimals
            "metadata": {
                "spec": "ft-1.0.0",
                "name": "Test Token",
                "symbol": "TEST",
                "decimals": 18
            }
        }))
        .gas(Gas::tgas(50))
        .await?;

    Ok((ft_id, ft_key))
}

#[tokio::test]
async fn test_ft_metadata() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    // Test FT metadata
    let ft = root_near.ft(&ft_id).unwrap();
    let metadata = ft.metadata().await.unwrap();

    assert_eq!(metadata.name, "Test Token");
    assert_eq!(metadata.symbol, "TEST");
    assert_eq!(metadata.decimals, 18);
    assert_eq!(metadata.spec, "ft-1.0.0");

    println!("FT Metadata: {} ({})", metadata.name, metadata.symbol);
}

#[tokio::test]
async fn test_ft_balance_of() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account (will receive initial supply)
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    // Test balance query
    let ft = root_near.ft(&ft_id).unwrap();
    let balance = ft.balance_of(&owner_id).await.unwrap();

    println!("Owner balance: {}", balance);
    println!("Raw: {}", balance.raw());

    // Owner should have initial supply
    assert_eq!(
        balance.raw(),
        1_000_000_000_000_000_000_000_000_u128,
        "Owner should have 1M tokens"
    );
    assert_eq!(balance.symbol(), "TEST");
    assert_eq!(balance.decimals(), 18);

    // Check display formatting
    assert_eq!(format!("{}", balance), "1000000 TEST");
}

#[tokio::test]
async fn test_ft_total_supply() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    // Test total supply
    let ft = root_near.ft(&ft_id).unwrap();
    let supply = ft.total_supply().await.unwrap();

    println!("Total supply: {}", supply);
    assert_eq!(supply.raw(), 1_000_000_000_000_000_000_000_000_u128);
}

#[tokio::test]
async fn test_ft_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create owner account (sender)
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create receiver account - must be created by owner, not root
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", owner_id).parse().unwrap();

    let owner_near = Near::custom(rpc_url)
        .credentials(owner_key.to_string(), owner_id.as_str())
        .unwrap()
        .build();

    owner_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    // Get FT client with signer
    let ft = owner_near.ft(&ft_id).unwrap();

    // First, register receiver for storage
    ft.storage_deposit(&receiver_id)
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check receiver is registered
    assert!(ft.is_registered(&receiver_id).await.unwrap());

    // Get initial balances
    let owner_balance_before = ft.balance_of(&owner_id).await.unwrap();
    let receiver_balance_before = ft.balance_of(&receiver_id).await.unwrap();

    println!("Before transfer:");
    println!("  Owner: {}", owner_balance_before);
    println!("  Receiver: {}", receiver_balance_before);

    // Transfer tokens
    let transfer_amount = 100_000_000_000_000_000_000_u128; // 100 tokens (18 decimals)

    ft.transfer_with_memo(&receiver_id, transfer_amount, "Test transfer")
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check balances after
    let owner_balance_after = ft.balance_of(&owner_id).await.unwrap();
    let receiver_balance_after = ft.balance_of(&receiver_id).await.unwrap();

    println!("After transfer:");
    println!("  Owner: {}", owner_balance_after);
    println!("  Receiver: {}", receiver_balance_after);

    // Verify balances changed correctly
    assert_eq!(
        owner_balance_after.raw(),
        owner_balance_before.raw() - transfer_amount
    );
    assert_eq!(
        receiver_balance_after.raw(),
        receiver_balance_before.raw() + transfer_amount
    );
}

#[tokio::test]
async fn test_ft_storage_deposit() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create owner's near client for signing
    let owner_near = Near::custom(rpc_url)
        .credentials(owner_key.to_string(), owner_id.as_str())
        .unwrap()
        .build();

    // Create account to register - must be created by owner
    let user_key = SecretKey::generate_ed25519();
    let user_id: AccountId = format!("user.{}", owner_id).parse().unwrap();

    owner_near
        .transaction(&user_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(user_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    let ft = owner_near.ft(&ft_id).unwrap();

    // User should not be registered initially
    assert!(!ft.is_registered(&user_id).await.unwrap());

    // Register user
    let balance = ft
        .storage_deposit(&user_id)
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("Storage balance after deposit: {:?}", balance);

    // User should now be registered
    assert!(ft.is_registered(&user_id).await.unwrap());

    // Check storage balance
    let storage = ft.storage_balance_of(&user_id).await.unwrap();
    assert!(storage.is_some());
    println!("User storage: {:?}", storage);
}

// =============================================================================
// NFT Tests
// =============================================================================

/// Deploy an NFT contract and return the contract account ID
async fn deploy_nft_contract(
    near: &Near,
    owner_id: &AccountId,
) -> Result<(AccountId, SecretKey), Error> {
    let nft_key = SecretKey::generate_ed25519();
    let nft_id = unique_account();

    let wasm = std::fs::read("tests/contracts/nft.wasm").expect("nft.wasm not found");

    // Create NFT contract account with wasm deployed
    near.transaction(&nft_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(nft_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await?;

    // Initialize the NFT contract
    let nft_near = Near::custom(near.rpc_url())
        .credentials(nft_key.to_string(), nft_id.as_str())?
        .build();

    nft_near
        .call(&nft_id, "new")
        .args(serde_json::json!({
            "owner_id": owner_id.to_string(),
            "metadata": {
                "spec": "nft-1.0.0",
                "name": "Test NFT Collection",
                "symbol": "TNFT"
            }
        }))
        .gas(Gas::tgas(50))
        .await?;

    Ok((nft_id, nft_key))
}

/// Mint an NFT token
async fn mint_nft(
    near: &Near,
    nft_id: &AccountId,
    nft_key: &SecretKey,
    token_id: &str,
    owner_id: &AccountId,
) -> Result<(), Error> {
    let nft_near = Near::custom(near.rpc_url())
        .credentials(nft_key.to_string(), nft_id.as_str())?
        .build();

    nft_near
        .call(nft_id, "nft_mint")
        .args(serde_json::json!({
            "token_id": token_id,
            "receiver_id": owner_id.to_string(),
            "token_metadata": {
                "title": format!("Token #{}", token_id),
                "description": "A test NFT",
                "media": null
            }
        }))
        .gas(Gas::tgas(50))
        .deposit(NearToken::millinear(10)) // Storage deposit for minting
        .await?;

    Ok(())
}

#[tokio::test]
async fn test_nft_metadata() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, _nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    // Test NFT metadata
    let nft = root_near.nft(&nft_id).unwrap();
    let metadata = nft.metadata().await.unwrap();

    assert_eq!(metadata.name, "Test NFT Collection");
    assert_eq!(metadata.symbol, "TNFT");
    assert_eq!(metadata.spec, "nft-1.0.0");

    println!("NFT Metadata: {} ({})", metadata.name, metadata.symbol);
}

#[tokio::test]
async fn test_nft_token_query() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    // Mint a token
    mint_nft(&root_near, &nft_id, &nft_key, "token-1", &owner_id)
        .await
        .unwrap();

    // Query the token
    let nft = root_near.nft(&nft_id).unwrap();
    let token = nft.token("token-1").await.unwrap();

    assert!(token.is_some(), "Token should exist");
    let token = token.unwrap();

    assert_eq!(token.token_id, "token-1");
    assert_eq!(token.owner_id, owner_id.to_string());

    if let Some(meta) = &token.metadata {
        assert_eq!(meta.title, Some("Token #token-1".to_string()));
        println!("Token title: {:?}", meta.title);
    }

    // Query non-existent token
    let missing = nft.token("nonexistent").await.unwrap();
    assert!(missing.is_none(), "Non-existent token should return None");
}

#[tokio::test]
async fn test_nft_tokens_for_owner() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    // Mint multiple tokens
    for i in 1..=5 {
        mint_nft(
            &root_near,
            &nft_id,
            &nft_key,
            &format!("token-{}", i),
            &owner_id,
        )
        .await
        .unwrap();
    }

    // Query tokens for owner
    let nft = root_near.nft(&nft_id).unwrap();
    let tokens = nft.tokens_for_owner(&owner_id, None, None).await.unwrap();

    assert_eq!(tokens.len(), 5, "Owner should have 5 tokens");

    for token in &tokens {
        println!("Token: {} owned by {}", token.token_id, token.owner_id);
        assert_eq!(token.owner_id, owner_id.to_string());
    }

    // Test pagination
    let first_two = nft
        .tokens_for_owner(&owner_id, None, Some(2))
        .await
        .unwrap();
    assert_eq!(first_two.len(), 2);

    let next_two = nft
        .tokens_for_owner(&owner_id, Some(2), Some(2))
        .await
        .unwrap();
    assert_eq!(next_two.len(), 2);
}

#[tokio::test]
async fn test_nft_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create owner's client for signing
    let owner_near = Near::custom(rpc_url)
        .credentials(owner_key.to_string(), owner_id.as_str())
        .unwrap()
        .build();

    // Create receiver account - must be created by owner
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", owner_id).parse().unwrap();

    owner_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    // Mint a token to owner
    mint_nft(&root_near, &nft_id, &nft_key, "transfer-test", &owner_id)
        .await
        .unwrap();

    // Verify owner owns the token
    let nft = root_near.nft(&nft_id).unwrap();
    let token = nft.token("transfer-test").await.unwrap().unwrap();
    assert_eq!(token.owner_id, owner_id.to_string());

    // Transfer the token
    let nft_with_signer = owner_near.nft(&nft_id).unwrap();
    nft_with_signer
        .transfer_with_memo(&receiver_id, "transfer-test", "Gift for you!")
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify receiver now owns the token
    let token = nft.token("transfer-test").await.unwrap().unwrap();
    assert_eq!(token.owner_id, receiver_id.to_string());

    println!("Token transferred successfully to {}", receiver_id);
}

#[tokio::test]
async fn test_nft_total_supply() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    let nft = root_near.nft(&nft_id).unwrap();

    // Initial supply should be 0
    let supply = nft.total_supply().await.unwrap();
    assert_eq!(supply, 0);

    // Mint some tokens
    for i in 1..=3 {
        mint_nft(
            &root_near,
            &nft_id,
            &nft_key,
            &format!("supply-{}", i),
            &owner_id,
        )
        .await
        .unwrap();
    }

    // Supply should now be 3
    let supply = nft.total_supply().await.unwrap();
    assert_eq!(supply, 3);

    println!("Total NFT supply: {}", supply);
}

#[tokio::test]
async fn test_nft_supply_for_owner() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create first owner account
    let owner1_key = SecretKey::generate_ed25519();
    let owner1_id = unique_account();

    root_near
        .transaction(&owner1_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner1_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create owner1's client for signing
    let owner1_near = Near::custom(rpc_url)
        .credentials(owner1_key.to_string(), owner1_id.as_str())
        .unwrap()
        .build();

    // Create second owner as subaccount of owner1
    let owner2_key = SecretKey::generate_ed25519();
    let owner2_id: AccountId = format!("owner2.{}", owner1_id).parse().unwrap();

    owner1_near
        .transaction(&owner2_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(owner2_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy NFT contract
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner1_id).await.unwrap();

    // Mint tokens to different owners
    for i in 1..=3 {
        mint_nft(
            &root_near,
            &nft_id,
            &nft_key,
            &format!("owner1-{}", i),
            &owner1_id,
        )
        .await
        .unwrap();
    }

    for i in 1..=2 {
        mint_nft(
            &root_near,
            &nft_id,
            &nft_key,
            &format!("owner2-{}", i),
            &owner2_id,
        )
        .await
        .unwrap();
    }

    let nft = root_near.nft(&nft_id).unwrap();

    // Check supply per owner
    let owner1_supply = nft.supply_for_owner(&owner1_id).await.unwrap();
    let owner2_supply = nft.supply_for_owner(&owner2_id).await.unwrap();

    assert_eq!(owner1_supply, 3);
    assert_eq!(owner2_supply, 2);

    println!(
        "Owner1 has {} tokens, Owner2 has {} tokens",
        owner1_supply, owner2_supply
    );
}

// =============================================================================
// FtAmount Integration Tests
// =============================================================================

#[tokio::test]
async fn test_ft_amount_arithmetic_from_real_balances() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    let ft = root_near.ft(&ft_id).unwrap();
    let balance = ft.balance_of(&owner_id).await.unwrap();
    let supply = ft.total_supply().await.unwrap();

    // Should be able to do arithmetic on same-token amounts
    let sum = balance.checked_add(&supply);
    assert!(sum.is_some(), "Adding same token should work");

    let diff = supply.checked_sub(&balance);
    assert!(diff.is_some(), "Subtracting same token should work");
    assert!(diff.unwrap().is_zero(), "Supply - balance should be 0");

    println!("Balance: {}", balance);
    println!("Supply: {}", supply);
    println!("Balance + Supply: {:?}", sum.map(|a| a.to_string()));
}

// =============================================================================
// Metadata Caching Tests
// =============================================================================

#[tokio::test]
async fn test_ft_metadata_caching() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create owner account
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account();

    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let (ft_id, _ft_key) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    let ft = root_near.ft(&ft_id).unwrap();

    // First call fetches metadata
    let meta1 = ft.metadata().await.unwrap();
    println!("First fetch: {}", meta1.name);

    // Second call should use cache (same pointer)
    let meta2 = ft.metadata().await.unwrap();
    assert!(
        std::ptr::eq(meta1, meta2),
        "Second call should return cached reference"
    );

    // Multiple balance_of calls should also use cached metadata
    let b1 = ft.balance_of(&owner_id).await.unwrap();
    let b2 = ft.balance_of(&owner_id).await.unwrap();

    assert_eq!(b1.symbol(), b2.symbol());
    assert_eq!(b1.decimals(), b2.decimals());

    println!("Caching works correctly");
}
