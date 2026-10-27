//! RPC error shapes that are observable only against a real node.

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, RpcError};
use near_kit::transaction::{ContractError, Final};
use near_kit::{AccountId, CryptoHash, Error, NearToken};
use near_kit_sandbox::SANDBOX_ROOT_ACCOUNT;

use super::support::{funded_account, guestbook_wasm, shared_client};

#[tokio::test]
async fn missing_account_returns_typed_error() {
    let (_, near) = shared_client().await;
    let account_id: AccountId = "definitely-does-not-exist.sandbox"
        .parse()
        .expect("valid missing account ID");

    let error = near
        .near()
        .balance(&account_id)
        .await
        .expect_err("missing account must fail");
    assert!(matches!(
        error,
        Error::Rpc(error)
            if matches!(error.as_ref(), RpcError::AccountNotFound { account_id: actual, .. } if actual == &account_id)
    ));
}

#[tokio::test]
async fn missing_account_access_keys_are_empty() {
    let (_, near) = shared_client().await;
    let account_id: AccountId = "missing-access-keys.sandbox"
        .parse()
        .expect("valid missing account ID");

    let keys = near
        .near()
        .rpc()
        .view_access_key_list(&account_id, Default::default())
        .await
        .expect("the node represents missing-account keys as an empty list");
    assert!(keys.keys.is_empty());
    assert!(
        !near
            .near()
            .account_exists(&account_id)
            .await
            .expect("account existence normalizes AccountNotFound")
    );
}

#[tokio::test]
async fn account_without_code_returns_contract_not_deployed() {
    let (_, near) = shared_client().await;

    let error = near
        .near()
        .view::<serde_json::Value>(
            SANDBOX_ROOT_ACCOUNT.parse::<AccountId>().unwrap(),
            "get_greeting",
        )
        .args(())
        .await
        .expect_err("root account has no contract");
    assert!(matches!(
        error,
        ContractError::Backend(RpcError::ContractNotDeployed { .. })
    ));
}

#[tokio::test]
async fn missing_contract_method_returns_typed_error() {
    let (sandbox, root) = shared_client().await;
    let (contract, contract_id, _) =
        funded_account(&root, sandbox, "errmethod", NearToken::from_near(10)).await;
    contract
        .wait_until::<Final>()
        .transaction(contract.account_id())
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .expect("guestbook deployment");

    let error = root
        .near()
        .view::<serde_json::Value>(&contract_id, "this_method_does_not_exist")
        .args(())
        .await
        .expect_err("missing method must fail");
    assert!(matches!(
        error,
        ContractError::Backend(RpcError::MethodNotFound { contract_id: actual, method_name, .. })
            if actual == contract_id && method_name == "this_method_does_not_exist"
    ));
}

#[tokio::test]
async fn invalid_block_hash_returns_unknown_block() {
    let (_, near) = shared_client().await;

    let error = near
        .near()
        .rpc()
        .block(BlockReference::from(CryptoHash::default()))
        .await
        .expect_err("zero block hash must not exist");
    assert!(matches!(error, RpcError::UnknownBlock(_)));
}
