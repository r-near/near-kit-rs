//! Integration tests for delegate actions (meta-transactions, NEP-366): a
//! sender signs a delegate action, a relayer submits it, and it executes.

use near_kit::protocol::Action;
use near_kit::protocol::SignedDelegateAction;
use near_kit::transaction::{DelegateOptions, Final};
use near_kit::*;

use super::support::funded_account;

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
