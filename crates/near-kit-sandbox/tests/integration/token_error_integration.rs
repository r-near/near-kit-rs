//! Integration tests for token helper error paths.
//!
//! These tests verify that FT and NFT helpers handle errors correctly.

use near_kit::ContractError;
use near_kit::protocol::Action;
use near_kit::standards::{FungibleToken, NonFungibleToken};
use near_kit::*;
use near_kit::{rpc::RpcError, signer::SecretKey, transaction::Final};

use super::support::{funded_account, guestbook_wasm, shared_client, unique_account};

fn assert_contract_not_deployed(error: ContractError<RpcError>, expected_account: &AccountId) {
    match error {
        ContractError::Backend(RpcError::ContractNotDeployed { account_id, .. }) => {
            assert_eq!(&account_id, expected_account);
        }
        other => panic!("expected ContractNotDeployed, got {other:?}"),
    }
}

fn assert_method_not_found(
    error: ContractError<RpcError>,
    expected_contract: &AccountId,
    expected_method: &str,
) {
    match error {
        ContractError::Backend(RpcError::MethodNotFound {
            contract_id,
            method_name,
            ..
        }) => {
            assert_eq!(&contract_id, expected_contract);
            assert_eq!(method_name, expected_method);
        }
        other => panic!("expected MethodNotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn test_token_queries_on_non_contract_account() {
    let (sandbox, near) = shared_client().await;
    let (_, account_id, _) =
        funded_account(&near, sandbox, "tokerr", NearToken::from_near(10)).await;

    let ft = near.contract::<FungibleToken>(&account_id);
    assert_contract_not_deployed(ft.metadata().await.unwrap_err(), &account_id);
    assert_contract_not_deployed(
        ft.balance_of(account!("alice.near")).await.unwrap_err(),
        &account_id,
    );

    let nft = near.contract::<NonFungibleToken>(&account_id);
    assert_contract_not_deployed(nft.metadata().await.unwrap_err(), &account_id);
    assert_contract_not_deployed(nft.token("any-token").await.unwrap_err(), &account_id);
}

#[tokio::test]
async fn test_ft_metadata_on_nonexistent_account() {
    let (_sandbox, near) = shared_client().await;

    let ft = near.contract::<FungibleToken>(account!("nonexistent-ft-contract.sandbox"));
    let error = ft.metadata().await.unwrap_err();
    assert!(matches!(
        error,
        ContractError::Backend(RpcError::AccountNotFound { .. })
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
        .unwrap()
        .result()
        .unwrap();

    let ft = near.contract::<FungibleToken>(&contract_id);
    assert_method_not_found(
        ft.metadata().await.unwrap_err(),
        &contract_id,
        "ft_metadata",
    );

    let nft = near.contract::<NonFungibleToken>(&contract_id);
    assert_method_not_found(
        nft.metadata().await.unwrap_err(),
        &contract_id,
        "nft_metadata",
    );
}
