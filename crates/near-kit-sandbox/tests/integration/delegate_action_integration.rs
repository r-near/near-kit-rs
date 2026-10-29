//! Integration tests for delegate actions (meta-transactions, NEP-366): a
//! sender signs a delegate action, a relayer submits it, and it executes.

use near_kit::protocol::Action;
use near_kit::protocol::SignedDelegateAction;
use near_kit::transaction::{DelegateOptions, Final, FunctionCall};
use near_kit::*;

use super::support::{funded_account, guestbook_wasm};

#[tokio::test]
async fn test_delegate_action_transfer() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // The sender transfers without paying gas; the relayer pays.
    let (sender_near, sender_id, _) =
        funded_account(&root_near, sandbox, "sender", NearToken::from_near(10)).await;
    let (relayer_near, _, _) =
        funded_account(&root_near, sandbox, "relayer", NearToken::from_near(10)).await;
    let (_, recipient_id, _) =
        funded_account(&root_near, sandbox, "recipient", NearToken::from_near(1)).await;

    let initial_balance = root_near.near().balance(&recipient_id).await.unwrap();

    let delegate_result = sender_near
        .sign_delegate(
            &recipient_id,
            vec![Action::transfer(NearToken::from_near(2))],
            DelegateOptions {
                block_height_offset: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Decode the payload (simulating receiving it via HTTP)
    let signed_delegate = SignedDelegateAction::from_base64(&delegate_result.to_base64()).unwrap();
    assert_eq!(signed_delegate.sender_id().as_str(), sender_id.as_str());
    assert_eq!(
        signed_delegate.receiver_id().as_str(),
        recipient_id.as_str()
    );

    relayer_near
        .wait_until::<Final>()
        .transaction(signed_delegate.sender_id())
        .add_action(Action::delegate(signed_delegate))
        .await
        .unwrap()
        .result()
        .unwrap();

    let final_balance = root_near.near().balance(&recipient_id).await.unwrap();
    let diff = final_balance.total.as_yoctonear() - initial_balance.total.as_yoctonear();
    let expected = NearToken::from_near(2).as_yoctonear();
    assert_eq!(diff, expected, "Expected +2 NEAR, got diff: {} yocto", diff);
}

#[tokio::test]
async fn test_delegate_action_function_call() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (sender_near, sender_id, _) =
        funded_account(&root_near, sandbox, "sender", NearToken::from_near(10)).await;
    let (relayer_near, _, _) =
        funded_account(&root_near, sandbox, "relayer", NearToken::from_near(10)).await;
    let (contract_near, contract_id, _) =
        funded_account(&root_near, sandbox, "guestbook", NearToken::from_near(5)).await;
    contract_near
        .wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let relay = |call: FunctionCall| {
        let sender_near = &sender_near;
        let relayer_near = &relayer_near;
        let contract_id = &contract_id;
        async move {
            let signed = sender_near
                .sign_delegate(
                    contract_id,
                    vec![call.gas(Gas::from_tgas(30)).try_into().unwrap()],
                    DelegateOptions {
                        block_height_offset: Some(200),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            // The relayer receives the delegate action as base64 over HTTP.
            let signed = SignedDelegateAction::from_base64(&signed.to_base64()).unwrap();
            relayer_near
                .wait_until::<Final>()
                .transaction(signed.sender_id())
                .add_action(Action::delegate(signed))
                .await
                .unwrap()
        }
    };

    // The inner call runs as the sender.
    let outcome = relay(
        FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": "Hello from delegate!" })),
    )
    .await;
    outcome.result().unwrap();
    assert_eq!(outcome.receipt_failures().count(), 0);
    let messages: Vec<serde_json::Value> = root_near
        .near()
        .view(&contract_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["text"], "Hello from delegate!");
    assert_eq!(messages[0]["sender"], sender_id.as_str());

    // A failing inner call doesn't fail the relayer's transaction; only
    // `receipt_failures()` shows it.
    let outcome = relay(FunctionCall::new("no_such_method")).await;
    assert!(!outcome.is_failure());
    outcome.result().unwrap();
    assert_eq!(outcome.receipt_failures().count(), 1);
}
