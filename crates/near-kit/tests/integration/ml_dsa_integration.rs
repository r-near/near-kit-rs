//! ML-DSA-65 (FIPS 204, protocol v85) integration tests against a 2.13 sandbox.
//!
//! This is the definitive correctness proof for the hand-rolled ML-DSA-65
//! crypto and borsh: a key derived from a 32-byte seed is added on-chain and
//! used to sign a transfer that the node accepts.
//!
//! # Sandbox image
//!
//! The default `2.13.0-rc.2` image reports protocol v85 but was cut before the
//! ML-DSA (`PostQuantumSignatures`) code merged — it rejects key type 2 at
//! borsh decode. We therefore run this test against the `pre-release` tag,
//! which is the same v85 protocol line *with* the ML-DSA implementation. Once a
//! 2.13 RC that includes ML-DSA is published, this override can be dropped.

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, Sandbox, SandboxConfig};
use near_kit::*;

/// Sandbox image tag that includes the ML-DSA-65 implementation at protocol v85.
const ML_DSA_SANDBOX_VERSION: &str = "pre-release";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mldsa{}.{}", n, SANDBOX_ROOT_ACCOUNT)
        .parse()
        .unwrap()
}

/// A fresh sandbox running an ML-DSA-capable node.
async fn ml_dsa_sandbox() -> Sandbox {
    SandboxConfig::builder()
        .version(ML_DSA_SANDBOX_VERSION)
        .fresh()
        .await
}

/// Add an ML-DSA-65 full-access key on account creation, then sign a transfer
/// with that key. If the node accepts the transfer, our ML-DSA-65 public key,
/// signature, and borsh wire format are all correct against protocol v85.
#[tokio::test]
async fn test_ml_dsa65_signed_transfer_accepted_on_chain() {
    let sandbox = ml_dsa_sandbox().await;
    let near = sandbox.client();

    // Deterministic ML-DSA-65 key from a fixed 32-byte seed.
    let ml_dsa_key = SecretKey::ml_dsa65_from_seed([42u8; 32]);
    let public_key = ml_dsa_key.public_key();
    assert_eq!(public_key.key_type(), KeyType::MlDsa65);
    assert!(public_key.to_string().starts_with("ml-dsa-65:"));

    let account_id = unique_account();

    // Create the account with the ML-DSA-65 key as its only full-access key.
    // This exercises AddKey with a 1952-byte [2][..] borsh pubkey.
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(20))
        .add_full_access_key(public_key.clone())
        .send()
        .wait_until(Final)
        .await
        .expect("create_account with ML-DSA-65 key must be accepted");

    // The view RPC returns the key as an `ml-dsa-65-hash:` handle.
    let keys = near.access_keys(&account_id).await.unwrap();
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

    // Now sign a transfer FROM this account using the ML-DSA-65 secret key.
    // Acceptance proves the node verified an ML-DSA-65 signature.
    let recipient = unique_account();
    let recipient_key = SecretKey::generate_ed25519();
    near.transaction(&recipient)
        .create_account()
        .transfer(NearToken::from_near(1))
        .add_full_access_key(recipient_key.public_key())
        .send()
        .wait_until(Final)
        .await
        .unwrap();

    let signed = Near::sandbox(&sandbox)
        .with_signer(InMemorySigner::new(&account_id, ml_dsa_key.to_string()).unwrap());

    let before = near.balance(&recipient).await.unwrap().total;

    signed
        .transaction(&recipient)
        .transfer(NearToken::from_near(5))
        .send()
        .wait_until(Final)
        .await
        .expect("ML-DSA-65-signed transfer must be accepted on-chain");

    let after = near.balance(&recipient).await.unwrap().total;
    assert_eq!(
        after,
        before.saturating_add(NearToken::from_near(5)),
        "recipient should have received the ML-DSA-65-signed transfer"
    );
}
