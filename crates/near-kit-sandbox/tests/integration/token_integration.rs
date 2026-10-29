//! Integration tests for token helpers (FT and NFT).

use near_kit::protocol::Action;
use near_kit::standards::{FungibleToken, NonFungibleToken};
use near_kit::transaction::FunctionCall;
use near_kit::*;
use near_kit::{signer::*, transaction::Final};

use super::support::{deploy_ft_contract, funded_account, nft_wasm, shared_client, unique_account};

#[tokio::test]
async fn test_ft_metadata_balance_and_supply() {
    let (sandbox, root_near) = shared_client().await;
    let (_, owner_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;
    let (ft_id, _) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    let ft = root_near.contract::<FungibleToken>(&ft_id);
    let metadata = ft.metadata().await.unwrap();
    assert_eq!(metadata.name, "Test Token");
    assert_eq!(metadata.symbol, "TEST");
    assert_eq!(metadata.decimals, 18);
    assert_eq!(metadata.spec, "ft-1.0.0");

    let cached = ft.metadata().await.unwrap();
    assert!(
        std::ptr::eq(metadata, cached),
        "second metadata call should return the cached reference"
    );

    // The owner holds the whole initial supply: 1M tokens with 18 decimals
    let balance = ft.balance_of(&owner_id).await.unwrap();
    assert_eq!(
        balance, 1_000_000_000_000_000_000_000_000_u128,
        "Owner should have 1M tokens"
    );
    assert_eq!(ft.balance_of(&owner_id).await.unwrap(), balance);
    assert_eq!(ft.total_supply().await.unwrap(), balance);
}

#[tokio::test]
async fn test_ft_transfer() {
    let (sandbox, root_near) = shared_client().await;
    let (owner_near, owner_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;

    // Create receiver account - must be created by owner, not root
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", owner_id).parse().unwrap();

    owner_near
        .wait_until::<Final>()
        .transaction(&receiver_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(receiver_key.public_key()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let (ft_id, _) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();
    let ft = owner_near
        .wait_until::<Final>()
        .contract::<FungibleToken>(&ft_id);

    assert!(!ft.is_registered(&receiver_id).await.unwrap());

    let bounds = ft.storage_balance_bounds().await.unwrap();
    ft.storage_deposit(&receiver_id, bounds.min)
        .await
        .unwrap()
        .result()
        .unwrap();

    assert!(ft.is_registered(&receiver_id).await.unwrap());
    assert!(ft.storage_balance_of(&receiver_id).await.unwrap().is_some());

    let owner_balance_before = ft.balance_of(&owner_id).await.unwrap();
    let receiver_balance_before = ft.balance_of(&receiver_id).await.unwrap();

    let transfer_amount = 100_000_000_000_000_000_000_u128; // 100 tokens (18 decimals)
    ft.transfer(&receiver_id, transfer_amount, Some("Test transfer".into()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let owner_balance_after = ft.balance_of(&owner_id).await.unwrap();
    let receiver_balance_after = ft.balance_of(&receiver_id).await.unwrap();
    assert_eq!(owner_balance_after, owner_balance_before - transfer_amount);
    assert_eq!(
        receiver_balance_after,
        receiver_balance_before + transfer_amount
    );
}

/// Deploy an NFT contract and return the contract account ID
async fn deploy_nft_contract(
    near: &SigningNear,
    owner_id: &AccountId,
) -> Result<(AccountId, SecretKey), Error> {
    let nft_key = SecretKey::generate_ed25519();
    let nft_id = unique_account("token");

    near.wait_until::<Final>()
        .transaction(&nft_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(nft_key.public_key()))
        .add_action(Action::deploy_contract(nft_wasm()))
        .await?
        .result()?;

    let nft_near = near
        .near()
        .with_signer(InMemorySigner::new(&nft_id, nft_key.clone()));

    nft_near
        .transaction(&nft_id)
        .add_action(
            FunctionCall::new("new")
                .args(serde_json::json!({
                    "owner_id": owner_id.to_string(),
                    "metadata": {
                        "spec": "nft-1.0.0",
                        "name": "Test NFT Collection",
                        "symbol": "TNFT"
                    }
                }))
                .gas(Gas::from_tgas(50)),
        )
        .await?
        .result()?;

    Ok((nft_id, nft_key))
}

/// Mint an NFT token
async fn mint_nft(
    near: &SigningNear,
    nft_id: &AccountId,
    nft_key: &SecretKey,
    token_id: &str,
    owner_id: &AccountId,
) -> Result<(), Error> {
    let nft_near = near
        .near()
        .with_signer(InMemorySigner::new(nft_id, nft_key.clone()));

    nft_near
        .transaction(nft_id)
        .add_action(
            FunctionCall::new("nft_mint")
                .args(serde_json::json!({
                    "token_id": token_id,
                    "receiver_id": owner_id.to_string(),
                    "token_metadata": {
                        "title": format!("Token #{}", token_id),
                        "description": "A test NFT",
                        "media": null
                    }
                }))
                .gas(Gas::from_tgas(50))
                .deposit(NearToken::from_millinear(10)),
        ) // Storage deposit for minting
        .await?
        .result()?;

    Ok(())
}

#[tokio::test]
async fn test_nft_metadata_supply_and_token_query() {
    let (sandbox, root_near) = shared_client().await;
    let (_, owner_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

    let nft = root_near.contract::<NonFungibleToken>(&nft_id);
    let metadata = nft.metadata().await.unwrap();
    assert_eq!(metadata.name, "Test NFT Collection");
    assert_eq!(metadata.symbol, "TNFT");
    assert_eq!(metadata.spec, "nft-1.0.0");

    assert_eq!(nft.total_supply().await.unwrap(), 0);

    mint_nft(&root_near, &nft_id, &nft_key, "token-1", &owner_id)
        .await
        .unwrap();

    let token = nft
        .token("token-1")
        .await
        .unwrap()
        .expect("Token should exist");
    assert_eq!(token.token_id, "token-1");
    assert_eq!(token.owner_id, owner_id.to_string());
    if let Some(meta) = &token.metadata {
        assert_eq!(meta.title, Some("Token #token-1".to_string()));
    }

    let missing = nft.token("nonexistent").await.unwrap();
    assert!(missing.is_none(), "Non-existent token should return None");

    for i in 1..=2 {
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
    assert_eq!(nft.total_supply().await.unwrap(), 3);
}

#[tokio::test]
async fn test_nft_tokens_for_owner() {
    let (sandbox, root_near) = shared_client().await;
    let (_, owner_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;
    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();

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

    let nft = root_near.contract::<NonFungibleToken>(&nft_id);
    let tokens = nft.tokens_for_owner(&owner_id, None, None).await.unwrap();

    assert_eq!(tokens.len(), 5, "Owner should have 5 tokens");
    for token in &tokens {
        assert_eq!(token.owner_id, owner_id.to_string());
    }

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
    let (sandbox, root_near) = shared_client().await;
    let (owner_near, owner_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;

    // Create receiver account - must be created by owner
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", owner_id).parse().unwrap();

    owner_near
        .wait_until::<Final>()
        .transaction(&receiver_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(receiver_key.public_key()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner_id).await.unwrap();
    mint_nft(&root_near, &nft_id, &nft_key, "transfer-test", &owner_id)
        .await
        .unwrap();

    let nft = root_near.contract::<NonFungibleToken>(&nft_id);
    let token = nft.token("transfer-test").await.unwrap().unwrap();
    assert_eq!(token.owner_id, owner_id.to_string());

    let nft_with_signer = owner_near
        .wait_until::<Final>()
        .contract::<NonFungibleToken>(&nft_id);
    nft_with_signer
        .transfer(&receiver_id, "transfer-test", Some("Gift for you!".into()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let token = nft.token("transfer-test").await.unwrap().unwrap();
    assert_eq!(token.owner_id, receiver_id.to_string());
}

#[tokio::test]
async fn test_nft_supply_for_owner() {
    let (sandbox, root_near) = shared_client().await;
    let (owner1_near, owner1_id, _) =
        funded_account(&root_near, sandbox, "token", NearToken::from_near(100)).await;

    // Create second owner as subaccount of owner1
    let owner2_key = SecretKey::generate_ed25519();
    let owner2_id: AccountId = format!("owner2.{}", owner1_id).parse().unwrap();

    owner1_near
        .wait_until::<Final>()
        .transaction(&owner2_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(owner2_key.public_key()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let (nft_id, nft_key) = deploy_nft_contract(&root_near, &owner1_id).await.unwrap();

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

    let nft = root_near.contract::<NonFungibleToken>(&nft_id);
    assert_eq!(nft.supply_for_owner(&owner1_id).await.unwrap(), 3);
    assert_eq!(nft.supply_for_owner(&owner2_id).await.unwrap(), 2);
}
