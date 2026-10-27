//! ML-DSA-65 (FIPS 204, protocol v85) integration tests against a 2.13 sandbox.
//!
//! This is the definitive correctness proof for the hand-rolled ML-DSA-65
//! crypto and borsh: a key derived from a 32-byte seed is added on-chain and
//! used to sign a transfer that the node accepts.
//!
//! # Sandbox image
//!
//! Pins the sandbox to stable `2.13.4`. Protocol or ML-DSA incompatibility
//! fails the test rather than silently skipping its assertions.

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{signer::*, transaction::Final};

use super::support::{sandbox_2_13, unique_account};

/// Protocol version that ships ML-DSA-65 (NEAR 2.13 / v85).
const ML_DSA_PROTOCOL_VERSION: u32 = 85;

/// Add an ML-DSA-65 full-access key on account creation, then sign a transfer
/// with that key. If the node accepts the transfer, our ML-DSA-65 public key,
/// signature, and borsh wire format are all correct against protocol v85.
#[tokio::test]
async fn test_ml_dsa65_signed_transfer_accepted_on_chain() {
    let sandbox = sandbox_2_13().await;
    let near = sandbox.client();

    let status = near.near().rpc().status().await.expect("node status");
    assert!(
        status.protocol_version >= ML_DSA_PROTOCOL_VERSION,
        "stable sandbox must support ML-DSA protocol v{ML_DSA_PROTOCOL_VERSION}, got v{}",
        status.protocol_version
    );

    // Deterministic ML-DSA-65 key from a fixed 32-byte seed.
    let ml_dsa_key = SecretKey::ml_dsa65_from_seed([42u8; 32]);
    let public_key = ml_dsa_key.public_key();
    assert_eq!(public_key.key_type(), KeyType::MlDsa65);
    assert!(public_key.to_string().starts_with("ml-dsa-65:"));

    let account_id = unique_account("mldsa");

    // Create the account with the ML-DSA-65 key as its only full-access key.
    // This exercises AddKey with a 1952-byte [2][..] borsh pubkey.
    near.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(20)))
        .add_action(Action::add_full_access_key(public_key.clone()))
        .await
        .expect("create_account with ML-DSA-65 key must be accepted");

    // The view RPC returns the key as an `ml-dsa-65-hash:` handle.
    let keys = near
        .near()
        .rpc()
        .view_access_key_list(&account_id, near_kit::rpc::Finality::Final)
        .await
        .unwrap();
    assert_eq!(keys.keys.len(), 1);
    let listed = &keys.keys[0].public_key;
    assert_eq!(listed.key_type(), KeyType::MlDsa65);
    assert!(
        listed.is_ml_dsa65_hash(),
        "on-chain ML-DSA-65 access key should be a hash handle, got {listed}"
    );
    // The handle must equal the SHA3-256 handle of our full key.
    assert_eq!(
        Some(listed.clone()),
        public_key.to_ml_dsa65_hash(),
        "view handle must match locally-computed SHA3-256 handle"
    );
    assert!(listed.refers_to(&public_key));

    // Now sign a transfer FROM this account using the ML-DSA-65 secret key.
    // Acceptance proves the node verified an ML-DSA-65 signature.
    let recipient = unique_account("mldsa");
    let recipient_key = SecretKey::generate_ed25519();
    near.wait_until::<Final>()
        .transaction(&recipient)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .add_action(Action::add_full_access_key(recipient_key.public_key()))
        .await
        .unwrap();

    let signed = sandbox
        .client()
        .near()
        .with_signer(InMemorySigner::new(&account_id, ml_dsa_key.clone()));

    let before = near.near().balance(&recipient).await.unwrap().total;

    signed
        .wait_until::<Final>()
        .transaction(&recipient)
        .add_action(Action::transfer(NearToken::from_near(5)))
        .await
        .expect("ML-DSA-65-signed transfer must be accepted on-chain");

    let after = near.near().balance(&recipient).await.unwrap().total;
    assert_eq!(
        after,
        before.saturating_add(NearToken::from_near(5)),
        "recipient should have received the ML-DSA-65-signed transfer"
    );
}
