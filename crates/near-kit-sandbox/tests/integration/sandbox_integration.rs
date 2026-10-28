//! Docker lifecycle and sandbox-specific RPC integration tests.

use near_kit::NearToken;
use near_kit::protocol::Action;
use near_kit::rpc::SendTxResponse;
use near_kit::transaction::{Final, Included};
use near_kit_sandbox::Sandbox;

use super::support::{funded_account, guestbook_wasm, shared_client};

#[tokio::test]
async fn set_balance_preserves_other_account_fields() {
    let (sandbox, root) = shared_client().await;
    let (account, account_id, _) =
        funded_account(&root, sandbox, "patch", NearToken::from_near(50)).await;

    account
        .wait_until::<Final>()
        .transaction(account.account_id())
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .expect("guestbook deployment")
        .result()
        .unwrap();

    let original = root
        .near()
        .rpc()
        .view_account(&account_id, near_kit::rpc::Finality::Final)
        .await
        .expect("original account");
    let target_balance = NearToken::from_near(500_000);
    sandbox
        .set_balance(&account_id, target_balance)
        .await
        .expect("balance patch");

    let updated = root
        .near()
        .rpc()
        .view_account(&account_id, near_kit::rpc::Finality::Final)
        .await
        .expect("patched account");
    assert_eq!(updated.amount, target_balance);
    assert_eq!(updated.code_hash, original.code_hash);
    assert_eq!(updated.storage_usage, original.storage_usage);

    let messages: Vec<serde_json::Value> = account
        .near()
        .view(&account_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .expect("contract remains callable after patch");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn builder_wait_until_included_returns_send_response() {
    let (sandbox, root) = shared_client().await;
    let (sender, sender_id, _) =
        funded_account(&root, sandbox, "included", NearToken::from_near(20)).await;
    let (_, receiver_id, _) =
        funded_account(&root, sandbox, "includedrecv", NearToken::from_near(1)).await;

    let response: SendTxResponse = sender
        .wait_until::<Included>()
        .transaction(&receiver_id)
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await
        .expect("included transfer");

    assert!(!response.transaction_hash.is_zero());
    assert_eq!(response.sender_id, sender_id);
}

#[tokio::test]
async fn pre_signed_transaction_is_accepted() {
    let (sandbox, root) = shared_client().await;
    let (sender, _, _) =
        funded_account(&root, sandbox, "presigned", NearToken::from_near(20)).await;
    let (_, receiver_id, _) =
        funded_account(&root, sandbox, "presignedrecv", NearToken::from_near(1)).await;
    let before = root
        .near()
        .balance(&receiver_id)
        .await
        .expect("receiver balance");

    let signed = sender
        .sign(
            &receiver_id,
            vec![Action::transfer(NearToken::from_near(2))],
        )
        .await
        .expect("signed transaction");
    // Balance queries use final state, so wait for the transfer to become final.
    let outcome = sender
        .near()
        .send_signed(&signed)
        .wait_until::<Final>()
        .await
        .expect("pre-signed submission");

    assert!(outcome.is_success());
    let after = root
        .near()
        .balance(&receiver_id)
        .await
        .expect("receiver balance");
    assert_eq!(
        after.total.as_yoctonear() - before.total.as_yoctonear(),
        NearToken::from_near(2).as_yoctonear()
    );
}

#[tokio::test]
async fn custom_chain_id_reaches_the_client() {
    let sandbox = Sandbox::builder()
        .chain_id("pinet")
        .fresh()
        .await
        .expect("custom-chain sandbox");
    let near = sandbox.root_account();

    assert_eq!(near.near().chain_id().as_str(), "pinet");
    assert!(
        near.near()
            .balance(near_kit::account!("sandbox"))
            .await
            .expect("root balance")
            .total
            > NearToken::from_near(1)
    );
}

#[tokio::test]
async fn fast_forward_advances_block_height() {
    let sandbox = Sandbox::builder().fresh().await.expect("fresh sandbox");
    let near = sandbox.root_account();
    let before = near
        .near()
        .rpc()
        .status()
        .await
        .expect("status before fast-forward")
        .sync_info
        .latest_block_height;

    sandbox.fast_forward(100).await.expect("fast-forward");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let after = near
        .near()
        .rpc()
        .status()
        .await
        .expect("status after fast-forward")
        .sync_info
        .latest_block_height;
    assert!(after >= before + 100, "before={before}, after={after}");
}
