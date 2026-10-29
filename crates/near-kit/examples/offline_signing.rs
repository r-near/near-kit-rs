//! Offline signing: build, sign and encode a transaction with no client, and
//! sign and verify a NEP-413 message.
//!
//! Nothing here talks to the network. A signing service, a hardware-wallet
//! bridge or a cold machine can do all of it and hand the result to something
//! online. This example builds with no near-kit features (no RPC client).
//!
//! What it shows:
//!
//! - `Transaction::new(..)` with a nonce and block hash obtained elsewhere,
//!   signed with `Transaction::sign(&secret_key)`.
//! - `SignedTransaction::to_base64` / `from_base64`: the encoding to move a
//!   signed transaction between machines, and how the receiving side checks
//!   it before submitting.
//! - NEP-413: signing an off-chain message (a login challenge) and verifying
//!   it with `nep413::verify_signature`, including the JSON a frontend sends
//!   and the server's check that it answers its own, unused challenge.
//!
//! To submit the transaction from an online machine, decode it there and use
//! `Near::send_signed(&signed)`. To sign with a nonce and block hash fetched
//! for you, use `SigningNear::sign` instead.
//!
//! Run it:
//!
//! ```text
//! cargo run -p near-kit --example offline_signing
//! ```

use std::collections::HashSet;

use near_kit::protocol::{Action, SignedTransaction, Transaction};
use near_kit::signer::{InMemorySigner, SecretKey, Signer};
use near_kit::standards::nep413;
use near_kit::transaction::FunctionCall;
use near_kit::{CryptoHash, Gas, NearToken, account};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // In practice the key comes from your key store; here it's generated.
    let secret_key = SecretKey::generate_ed25519();
    let public_key = secret_key.public_key();

    // --- A transaction, built and signed offline ---

    // The two values that need the chain, fetched by whoever is online: the
    // access key's current nonce (`near.rpc().view_access_key(..)`) and a
    // recent block hash (`near.rpc().block(..)`); a transaction expires a
    // fixed number of blocks after its block hash. The transaction's nonce is
    // the key's nonce + 1.
    let access_key_nonce: u64 = 41;
    let block_hash = CryptoHash::ZERO;

    let tx = Transaction::new(
        account!("alice.testnet"),
        public_key.clone(),
        access_key_nonce + 1,
        account!("guestbook.testnet"),
        block_hash,
        vec![
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "signed offline" }))
                .gas(Gas::from_tgas(30))
                .try_into()?,
            Action::transfer(NearToken::from_millinear(10)),
        ],
    );
    let signed = tx.sign(&secret_key);
    println!("transaction hash: {}", signed.get_hash());

    // Borsh, then base64: what goes in a file, a QR code or an HTTP body.
    let encoded = signed.to_base64();
    println!("base64: {} chars", encoded.len());

    // --- The receiving side ---
    let decoded = SignedTransaction::from_base64(&encoded)?;
    assert_eq!(decoded, signed);

    // Check the signature against the transaction's own public key before
    // submitting: an edited transaction no longer matches.
    let hash = decoded.transaction.get_hash();
    assert!(
        decoded
            .signature
            .verify(hash.as_bytes(), &decoded.transaction.public_key)
    );
    println!(
        "decoded: {} -> {}, {} actions, signature ok",
        decoded.transaction.signer_id,
        decoded.transaction.receiver_id,
        decoded.transaction.actions.len()
    );

    let mut edited = decoded.clone();
    edited.transaction.receiver_id = account!("mallory.testnet");
    let edited_hash = edited.transaction.get_hash();
    assert!(
        !edited
            .signature
            .verify(edited_hash.as_bytes(), &edited.transaction.public_key)
    );
    println!("edited copy: signature rejected");

    // --- NEP-413: sign a message, not a transaction ---

    // The server issues a challenge and remembers its nonce; the user's
    // wallet signs it. A NEP-413 signature can't be replayed as a transaction.
    const APP: &str = "example.com";
    const LOGIN_MESSAGE: &str = "Sign in to example.com";
    // 32 bytes; near-kit's nonces start with a timestamp so verification can
    // reject old signatures.
    let challenge = nep413::generate_nonce();
    let mut issued = HashSet::from([challenge]);

    let alice = InMemorySigner::new(account!("alice.testnet"), secret_key);
    let params = nep413::SignMessageParams {
        message: LOGIN_MESSAGE.into(),
        recipient: APP.into(),
        nonce: challenge,
        callback_url: None,
        state: None,
    };
    let signed_message = alice.key().sign_nep413(alice.account_id(), &params).await?;

    // What a frontend POSTs to the server.
    let body = serde_json::to_string(&nep413::AuthPayload::from_signed(signed_message, &params))?;
    println!("auth payload: {body}");

    // The server parses it. The recipient, message and nonce come from the
    // request, so it first checks they are its own challenge: otherwise a
    // signature the user made for another app, or a replayed login, passes.
    // Then it verifies offline: the signature, and that the nonce is under 5
    // minutes old (`NonceValidation::default()`).
    let payload: nep413::AuthPayload = serde_json::from_str(&body)?;
    let is_our_challenge = |payload: &nep413::AuthPayload, issued: &HashSet<[u8; 32]>| {
        payload.recipient == APP
            && payload.message == LOGIN_MESSAGE
            && issued.contains(&payload.nonce)
    };
    assert!(is_our_challenge(&payload, &issued));
    let params = payload.to_params();
    let valid = nep413::verify_signature(
        &payload.signed_message,
        &params,
        nep413::NonceValidation::default(),
    );
    println!("NEP-413 signature valid: {valid}");
    assert!(valid);
    // Accepted: the nonce is used up, so the same payload can't log in again.
    issued.remove(&payload.nonce);
    assert!(!is_our_challenge(&payload, &issued));
    println!("same payload again: rejected (nonce already used)");

    // A signature for one recipient doesn't verify for another.
    let other_site = nep413::SignMessageParams {
        recipient: "other.example".into(),
        ..params
    };
    assert!(!nep413::verify_signature(
        &payload.signed_message,
        &other_site,
        nep413::NonceValidation::default()
    ));
    println!("same signature, other recipient: rejected");

    // `verify_signature` doesn't check that the key belongs to the account.
    // With the `rpc` feature, `nep413::verify(.., &near, ..)` also checks the
    // account has this key as a full-access key.

    println!("done");
    Ok(())
}
