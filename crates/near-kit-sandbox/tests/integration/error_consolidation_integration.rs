//! End-to-end outcome semantics that require a real node.

use near_kit::signer::SecretKey;
use near_kit::transaction::Final;
use near_kit::{NearToken, rpc::AccessKeyPermissionView};

use super::support::{funded_account, guestbook_wasm, shared_client};

#[tokio::test]
async fn failing_middle_action_rolls_back_prior_action() {
    let (sandbox, root) = shared_client().await;
    let (account, account_id, _) =
        funded_account(&root, sandbox, "errmid", NearToken::from_near(10)).await;
    let added_key = SecretKey::generate_ed25519().public_key();
    let missing_key = SecretKey::generate_ed25519().public_key();

    let outcome = account
        .transaction(&account_id)
        .add_full_access_key(added_key.clone())
        .delete_key(missing_key)
        .transfer(NearToken::from_near(1))
        .send()
        .wait_until::<Final>()
        .await
        .expect("action failures must return an inspectable outcome");

    assert!(outcome.is_failure());
    assert!(outcome.failure_message().is_some());

    let keys = root
        .access_keys(&account_id)
        .await
        .expect("access keys after failed transaction");
    assert!(
        keys.keys
            .iter()
            .all(|key| !key.public_key.refers_to(&added_key))
    );
    assert!(keys.keys.iter().all(|key| matches!(
        key.access_key.permission,
        AccessKeyPermissionView::FullAccess
    )));
}

#[tokio::test]
async fn failed_contract_call_preserves_receipts() {
    let (sandbox, root) = shared_client().await;
    let (contract, contract_id, _) =
        funded_account(&root, sandbox, "errreceipt", NearToken::from_near(10)).await;

    contract
        .deploy(guestbook_wasm())
        .send()
        .wait_until::<Final>()
        .await
        .expect("guestbook deployment");

    let outcome = contract
        .call(&contract_id, "nonexistent_method")
        .args(serde_json::json!({}))
        .await
        .expect("executed contract failures must return an outcome");

    assert!(outcome.is_failure());
    assert!(outcome.failure_message().is_some());
    assert!(!outcome.receipts_outcome.is_empty());
    assert!(!outcome.transaction_hash().is_zero());
}
