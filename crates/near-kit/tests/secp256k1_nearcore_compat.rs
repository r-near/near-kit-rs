//! Secp256k1 compatibility with nearcore.
//!
//! nearcore's `near-crypto` signs and verifies secp256k1 over the given bytes
//! as a 32-byte message (`secp256k1::Message::from_slice(data)`), with no
//! extra hashing. `nearcore_verify` below is a line-for-line port of
//! `near_crypto::Signature::verify` on the same libsecp256k1 version, and the
//! fixed vectors were produced by `near-crypto` / `near-primitives`
//! 0.38.0-rc.2 (which also verified the borsh `SignedTransaction` and
//! `SignedDelegateAction` built here).
#![cfg(not(target_arch = "wasm32"))]

use near_kit::protocol::{
    Action, DelegateAction, NonDelegateAction, SignedDelegateAction, SignedTransaction,
    Transaction, TransferAction,
};
use near_kit::signer::{PublicKey, SecretKey, Signature, SigningKey};
use near_kit::standards::nep413;
use near_kit::{CryptoHash, NearToken};

// Generated with near-crypto 0.38.0-rc.2 from secret bytes 0x01..=0x20.
const SECRET_KEY: &str = "secp256k1:4wBqpZM9xaSheZzJSMawUKKwhdpChKbZ5eu5ky4Vigw";
const PUBLIC_KEY: &str = "secp256k1:3ewF4NQgt5bPC4poahSay91pL76oniREy2B81BQuYVWbvuGLAFvttB6XioKNyczqKykPyXBuQ5Md2uGU2dJBXPL3";
/// `near_crypto::SecretKey::sign(sha256("near-kit secp256k1 test vector"))`.
const DIGEST_SIGNATURE: &str = "secp256k1:E95aMD7QDGySWyY1SstyTWwBLzmYTxxZeTmadYHiqs5dQYUSMkesh7UMCmswv6MwxB2CLr71mQZWforL8xtzeD9Sg";
const TX_HASH: &str = "c371520d02ea69ba73d2d3116eed0044bbb71a04535e6164cfdbf7cbf3ae2156";
/// `near_crypto::SecretKey::sign(TX_HASH)`.
const TX_SIGNATURE: &str = "secp256k1:4LiWEseXMWgx4dJdYX9GJdbfziktfyPRrSpHZYXMdBZr83iEDZ4CtmW8mj514ZwZvhzX4r8hmRrMPZf7fSJsKHaWk";
const DELEGATE_HASH: &str = "75afc0bae1fba2f0820a7999f8fc603a539dfa47e5172561475b607f808c7718";
/// `near_crypto::SecretKey::sign(DELEGATE_HASH)`.
const DELEGATE_SIGNATURE: &str = "secp256k1:FAewcqxJvB3sJ69PzmJnouZxa454AiLCj1qjAMdKng7U6d28HtdgusRz6tJM5QcWXmeYn48B48Vcv9sNDQnnkaDRi";

/// Port of nearcore `near_crypto::Signature::verify` for secp256k1.
fn nearcore_verify(signature: &Signature, data: &[u8], public_key: &PublicKey) -> bool {
    let (Signature::Secp256k1(sig), Some(pk)) = (signature, public_key.as_secp256k1_bytes()) else {
        return false;
    };
    let Ok(rec_id) = secp256k1::ecdsa::RecoveryId::from_i32(i32::from(sig[64])) else {
        return false;
    };
    let Ok(rsig) = secp256k1::ecdsa::RecoverableSignature::from_compact(&sig[..64], rec_id) else {
        return false;
    };
    let mut pdata = [4u8; 65];
    pdata[1..].copy_from_slice(pk);
    let Ok(message) = secp256k1::Message::from_slice(data) else {
        return false;
    };
    let Ok(pub_key) = secp256k1::PublicKey::from_slice(&pdata) else {
        return false;
    };
    secp256k1::Secp256k1::verification_only()
        .verify_ecdsa(&message, &rsig.to_standard(), &pub_key)
        .is_ok()
}

/// libsecp256k1 signing, as `near_crypto::SecretKey::sign` does it.
fn nearcore_sign(secret_key: &SecretKey, data: &[u8; 32]) -> Signature {
    let sk = secp256k1::SecretKey::from_slice(secret_key.as_bytes()).unwrap();
    let (rec_id, sig) = secp256k1::Secp256k1::signing_only()
        .sign_ecdsa_recoverable(&secp256k1::Message::from_slice(data).unwrap(), &sk)
        .serialize_compact();
    let mut bytes = [0u8; 65];
    bytes[..64].copy_from_slice(&sig);
    bytes[64] = rec_id.to_i32() as u8;
    Signature::secp256k1_from_bytes(bytes)
}

fn hash_hex(hash: &CryptoHash) -> String {
    hex(hash.as_bytes())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn vector_key() -> SecretKey {
    let key: SecretKey = SECRET_KEY.parse().unwrap();
    assert_eq!(key.public_key().to_string(), PUBLIC_KEY);
    key
}

fn transfer() -> Action {
    Action::Transfer(TransferAction {
        deposit: NearToken::from_near(1),
    })
}

#[test]
fn digest_signature_matches_near_crypto() {
    let key = vector_key();
    let digest = CryptoHash::hash(b"near-kit secp256k1 test vector");

    // Both sides use RFC 6979 nonces, so the signatures are byte-identical.
    let signature = key.sign(digest.as_bytes());
    assert_eq!(signature.to_string(), DIGEST_SIGNATURE);
    assert_eq!(nearcore_sign(&key, digest.as_bytes()), signature);

    let expected: Signature = DIGEST_SIGNATURE.parse().unwrap();
    assert!(expected.verify(digest.as_bytes(), &key.public_key()));
    assert!(nearcore_verify(
        &signature,
        digest.as_bytes(),
        &key.public_key()
    ));
}

#[test]
fn signed_transaction_verifies_like_nearcore() {
    let key = vector_key();
    let tx = Transaction::new(
        "alice.testnet".parse().unwrap(),
        key.public_key(),
        7,
        "bob.testnet".parse().unwrap(),
        CryptoHash::hash(b"block"),
        vec![transfer()],
    );
    assert_eq!(hash_hex(&tx.get_hash()), TX_HASH);

    let signed = tx.sign(&key);
    assert_eq!(signed.signature.to_string(), TX_SIGNATURE);

    // What a node sees: decode the borsh bytes, hash the transaction, verify.
    let decoded = SignedTransaction::from_bytes(&signed.to_bytes()).unwrap();
    let tx_hash = CryptoHash::hash(&borsh::to_vec(&decoded.transaction).unwrap());
    assert_eq!(hash_hex(&tx_hash), TX_HASH);
    assert!(nearcore_verify(
        &decoded.signature,
        tx_hash.as_bytes(),
        &decoded.transaction.public_key
    ));
}

#[test]
fn signed_delegate_action_verifies_like_nearcore() {
    let key = vector_key();
    let delegate = DelegateAction {
        sender_id: "alice.testnet".parse().unwrap(),
        receiver_id: "bob.testnet".parse().unwrap(),
        actions: vec![NonDelegateAction::from_action(transfer()).unwrap()],
        nonce: 1,
        max_block_height: 1000,
        public_key: key.public_key(),
    };
    let hash = delegate.get_hash();
    assert_eq!(hash_hex(&hash), DELEGATE_HASH);

    let signed = delegate.sign(key.sign(hash.as_bytes()));
    assert_eq!(signed.signature.to_string(), DELEGATE_SIGNATURE);

    let decoded = SignedDelegateAction::from_bytes(&signed.to_bytes()).unwrap();
    let da = &decoded.delegate_action;
    assert!(nearcore_verify(
        &decoded.signature,
        da.get_hash().as_bytes(),
        &da.public_key
    ));
}

#[tokio::test]
async fn signing_key_paths_verify_like_nearcore() {
    let key = vector_key();
    let signing_key = SigningKey::new(key.clone());

    // NEP-413: the signature is over the 32-byte SHA-256 of the tagged payload.
    let params = nep413::SignMessageParams {
        message: "Login".to_string(),
        recipient: "app.near".to_string(),
        nonce: [9; 32],
        callback_url: None,
        state: None,
    };
    let signed = signing_key
        .sign_nep413(&"alice.near".parse().unwrap(), &params)
        .await
        .unwrap();
    let hash = nep413::serialize_message(&params);
    assert!(nearcore_verify(
        &signed.signature,
        hash.as_bytes(),
        &key.public_key()
    ));
    assert!(nep413::verify_signature(
        &signed,
        &params,
        nep413::NonceValidation::None
    ));

    // Signing arbitrary-length bytes is an error, not a silently different hash.
    assert!(signing_key.sign(b"not a digest").await.is_err());
}

#[test]
fn random_keys_cross_verify() {
    for i in 0u8..32 {
        let key = SecretKey::generate_secp256k1();
        let digest = CryptoHash::hash(&[i; 7]);
        let ours = key.sign(digest.as_bytes());
        assert!(nearcore_verify(&ours, digest.as_bytes(), &key.public_key()));
        let theirs = nearcore_sign(&key, digest.as_bytes());
        assert!(theirs.verify(digest.as_bytes(), &key.public_key()));
        assert_eq!(ours, theirs);
    }
}

#[test]
fn double_hashed_signature_is_rejected() {
    // near-kit <= 0.18.2 signed sha256(tx_hash) instead of tx_hash (and
    // verified the same way, so it accepted its own signatures).
    let key = vector_key();
    let digest = CryptoHash::hash(b"near-kit secp256k1 test vector");
    let double = CryptoHash::hash(digest.as_bytes());
    let legacy = nearcore_sign(&key, double.as_bytes());
    assert!(legacy.verify(double.as_bytes(), &key.public_key()));

    assert!(!nearcore_verify(
        &legacy,
        digest.as_bytes(),
        &key.public_key()
    ));
    assert!(!legacy.verify(digest.as_bytes(), &key.public_key()));
}

#[test]
fn non_digest_input_is_rejected_on_verify() {
    let key = vector_key();
    let digest = CryptoHash::hash(b"near-kit secp256k1 test vector");
    let signature = key.sign(digest.as_bytes());

    // nearcore returns false when the data is not a 32-byte message.
    for len in [0usize, 31, 33, 64] {
        let data = vec![1u8; len];
        assert!(!signature.verify(&data, &key.public_key()));
        assert!(!nearcore_verify(&signature, &data, &key.public_key()));
    }
}

#[test]
#[should_panic(expected = "secp256k1 signing expects a 32-byte digest")]
fn non_digest_input_panics_on_sign() {
    vector_key().sign(b"not a digest");
}

#[test]
fn high_s_signature_is_rejected() {
    // n - s flips a valid low-S signature to its high-S twin; libsecp256k1
    // (and so nearcore) rejects it, and so must near-kit.
    const N: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];
    let key = vector_key();
    let digest = CryptoHash::hash(b"near-kit secp256k1 test vector");
    let Signature::Secp256k1(mut bytes) = key.sign(digest.as_bytes()) else {
        unreachable!()
    };
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let v = i16::from(N[i]) - i16::from(bytes[32 + i]) - borrow;
        bytes[32 + i] = v.rem_euclid(256) as u8;
        borrow = i16::from(v < 0);
    }
    bytes[64] ^= 1;
    let high_s = Signature::secp256k1_from_bytes(bytes);

    assert!(!nearcore_verify(
        &high_s,
        digest.as_bytes(),
        &key.public_key()
    ));
    assert!(!high_s.verify(digest.as_bytes(), &key.public_key()));
}
