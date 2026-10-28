//! Secp256k1 signing against a real node.
//!
//! nearcore verifies secp256k1 signatures over the 32-byte transaction (or
//! NEP-461 delegate-action) hash directly, with no further hashing. A node
//! accepting these transactions proves near-kit signs the same bytes.

use near_kit::protocol::Action;
use near_kit::protocol::SignedDelegateAction;
use near_kit::signer::{InMemorySigner, KeyType, SecretKey};
use near_kit::transaction::{DelegateOptions, Final};
use near_kit::*;

use super::support::unique_account;

#[tokio::test]
async fn test_secp256k1_signed_transfer_and_delegate_accepted_on_chain() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let secp_key = SecretKey::generate_secp256k1();
    assert_eq!(secp_key.key_type(), KeyType::Secp256k1);
    let sender_id = unique_account("secp");
    root_near
        .wait_until::<Final>()
        .transaction(&sender_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(secp_key.public_key()))
        .await
        .expect("create account with secp256k1 key")
        .result()
        .unwrap();

    let recipient_id = unique_account("secprecv");
    root_near
        .wait_until::<Final>()
        .transaction(&recipient_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .add_action(Action::add_full_access_key(
            SecretKey::generate_ed25519().public_key(),
        ))
        .await
        .unwrap()
        .result()
        .unwrap();

    let sender_near = sandbox
        .root_account()
        .near()
        .with_signer(InMemorySigner::new(&sender_id, secp_key.clone()));

    // Plain transaction signed by the secp256k1 key.
    let before = root_near.near().balance(&recipient_id).await.unwrap().total;
    sender_near
        .wait_until::<Final>()
        .transaction(&recipient_id)
        .add_action(Action::transfer(NearToken::from_near(2)))
        .await
        .expect("secp256k1-signed transfer must be accepted on-chain")
        .result()
        .unwrap();
    let after = root_near.near().balance(&recipient_id).await.unwrap().total;
    assert_eq!(after, before.saturating_add(NearToken::from_near(2)));

    // Delegate action signed by the secp256k1 key, relayed by root.
    let delegate = sender_near
        .sign_delegate(
            &recipient_id,
            vec![Action::transfer(NearToken::from_near(3))],
            DelegateOptions {
                block_height_offset: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let signed_delegate = SignedDelegateAction::from_base64(&delegate.to_base64()).unwrap();
    let da = &signed_delegate.delegate_action;
    assert!(
        signed_delegate
            .signature
            .verify(da.get_hash().as_bytes(), &da.public_key)
    );

    let outcome = root_near
        .wait_until::<Final>()
        .transaction(signed_delegate.sender_id())
        .add_action(Action::delegate(signed_delegate))
        .await
        .expect("relayed secp256k1 delegate action must be accepted");
    assert!(
        outcome.is_success(),
        "secp256k1 delegate action failed: {outcome:?}"
    );
    let relayed = root_near.near().balance(&recipient_id).await.unwrap().total;
    assert_eq!(relayed, after.saturating_add(NearToken::from_near(3)));
}
