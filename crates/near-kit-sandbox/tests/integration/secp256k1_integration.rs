//! Secp256k1 signing against a real node.
//!
//! nearcore verifies secp256k1 signatures over the 32-byte transaction (or
//! NEP-461 delegate-action) hash directly, with no further hashing. A node
//! accepting these transactions proves near-kit signs the same bytes.

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
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(secp_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create account with secp256k1 key");

    let recipient_id = unique_account("secprecv");
    root_near
        .transaction(&recipient_id)
        .create_account()
        .transfer(NearToken::from_near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .send()
        .wait_until::<Final>()
        .await
        .unwrap();

    let sender_near = Near::sandbox(sandbox)
        .with_signer(InMemorySigner::new(&sender_id, secp_key.to_string()).unwrap());

    // Plain transaction signed by the secp256k1 key.
    let before = root_near.balance(&recipient_id).await.unwrap().total;
    sender_near
        .transaction(&recipient_id)
        .transfer(NearToken::from_near(2))
        .send()
        .wait_until::<Final>()
        .await
        .expect("secp256k1-signed transfer must be accepted on-chain");
    let after = root_near.balance(&recipient_id).await.unwrap().total;
    assert_eq!(after, before.saturating_add(NearToken::from_near(2)));

    // Delegate action signed by the secp256k1 key, relayed by root.
    let delegate = sender_near
        .transaction(&recipient_id)
        .transfer(NearToken::from_near(3))
        .delegate(DelegateOptions::with_offset(200))
        .await
        .unwrap();
    let signed_delegate = SignedDelegateAction::from_base64(&delegate.payload).unwrap();
    let da = &signed_delegate.delegate_action;
    assert!(
        signed_delegate
            .signature
            .verify(da.get_hash().as_bytes(), &da.public_key)
    );

    let outcome = root_near
        .transaction(signed_delegate.sender_id())
        .signed_delegate_action(signed_delegate)
        .send()
        .wait_until::<Final>()
        .await
        .expect("relayed secp256k1 delegate action must be accepted");
    assert!(
        outcome.is_success(),
        "secp256k1 delegate action failed: {outcome:?}"
    );
    let relayed = root_near.balance(&recipient_id).await.unwrap().total;
    assert_eq!(relayed, after.saturating_add(NearToken::from_near(3)));
}
