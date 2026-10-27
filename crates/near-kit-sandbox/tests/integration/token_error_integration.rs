//! Integration tests for token helper error paths.
//!
//! These tests verify that FT and NFT helpers handle errors correctly.
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{rpc::RpcError, signer::SecretKey, transaction::Final};

use super::support::{guestbook_wasm, shared_client, unique_account};

fn assert_contract_not_deployed(error: Error, expected_account: &AccountId) {
    match error {
        Error::Rpc(error) => match error.as_ref() {
            RpcError::ContractNotDeployed { account_id, .. } => {
                assert_eq!(account_id, expected_account);
            }
            other => panic!("expected ContractNotDeployed, got {other:?}"),
        },
        other => panic!("expected RPC contract error, got {other:?}"),
    }
}

// =============================================================================
// FT Error Cases
// =============================================================================

#[tokio::test]
async fn test_ft_queries_on_non_contract_account() {
    let (_sandbox, near) = shared_client().await;

    // Create an account without any contract deployed
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account("tokerr");

    near.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(key.public_key()))
        .await
        .unwrap();

    // Try to get FT metadata from non-contract account
    let ft = near.ft(&account_id);
    let metadata_error = ft.metadata().await.unwrap_err();
    assert_contract_not_deployed(metadata_error, &account_id);

    let balance_error = ft.balance_of("alice.near").await.unwrap_err();
    assert_contract_not_deployed(balance_error, &account_id);
}

#[tokio::test]
async fn test_ft_metadata_on_nonexistent_account() {
    let (_sandbox, near) = shared_client().await;

    // Try to get FT metadata from non-existent account
    let ft = near.ft("nonexistent-ft-contract.sandbox"
        .parse::<AccountId>()
        .unwrap());
    let error = ft.metadata().await.unwrap_err();
    assert!(matches!(
        error,
        Error::Rpc(ref error) if matches!(error.as_ref(), RpcError::AccountNotFound { .. })
    ));
}

#[tokio::test]
async fn test_token_metadata_on_wrong_contract_type() {
    let (_sandbox, near) = shared_client().await;

    // Deploy the guestbook contract (not an FT)
    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account("tokerr");

    near.wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(key.public_key()))
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .unwrap();

    // Try to use FT methods on non-FT contract (no init needed)
    let ft = near.ft(&contract_id);
    let result = ft.metadata().await;

    assert!(result.is_err(), "Should error for non-FT contract");
    let err = result.unwrap_err();
    match err {
        Error::Rpc(e) => match e.as_ref() {
            RpcError::MethodNotFound {
                contract_id: actual_contract_id,
                method_name,
                ..
            } => {
                assert_eq!(actual_contract_id, &contract_id);
                assert_eq!(method_name, "ft_metadata");
            }
            other => panic!("Expected MethodNotFound, got: {other:?}"),
        },
        other => panic!("Expected RPC error, got: {other:?}"),
    }

    let nft = near.nft(&contract_id);
    let err = nft.metadata().await.unwrap_err();
    match err {
        Error::Rpc(error) => match error.as_ref() {
            RpcError::MethodNotFound {
                contract_id: actual_contract_id,
                method_name,
                ..
            } => {
                assert_eq!(actual_contract_id, &contract_id);
                assert_eq!(method_name, "nft_metadata");
            }
            other => panic!("Expected MethodNotFound, got: {other:?}"),
        },
        other => panic!("Expected RPC error, got: {other:?}"),
    }
}

// =============================================================================
// NFT Error Cases
// =============================================================================

#[tokio::test]
async fn test_nft_queries_on_non_contract() {
    let (_sandbox, near) = shared_client().await;

    // Create an account without any contract
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account("tokerr");

    near.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(key.public_key()))
        .await
        .unwrap();

    let nft = near.nft(&account_id);
    let metadata_error = nft.metadata().await.unwrap_err();
    assert_contract_not_deployed(metadata_error, &account_id);

    let token_error = nft.token("any-token").await.unwrap_err();
    assert_contract_not_deployed(token_error, &account_id);
}
