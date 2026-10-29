//! NEP-413: Off-chain message signing for authentication.
//!
//! The message types and the offline [`verify_signature()`] are re-exported
//! from `near-kit-primitives`; `verify()` (feature `rpc`) additionally checks key ownership
//! over RPC.
//!
//! [NEP-413](https://github.com/near/NEPs/blob/master/neps/nep-0413.md) enables users to
//! sign messages for authentication and ownership verification without gas fees or
//! blockchain transactions.
//!
//! # Example
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::{Near, account};
//! use near_kit::signer::{InMemorySigner, Signer};
//! use near_kit::standards::nep413;
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let signer = InMemorySigner::new(
//!     account!("alice.testnet"),
//!     "ed25519:...".parse()?,
//! );
//!
//! // Sign a message. A `SigningNear` signs the same way through
//! // `alice.signer().key()` and `alice.account_id()`.
//! let params = nep413::SignMessageParams {
//!     message: "Login to MyApp".to_string(),
//!     recipient: "myapp.com".to_string(),
//!     nonce: nep413::generate_nonce(),
//!     callback_url: None,
//!     state: None,
//! };
//!
//! let signed = signer.key().sign_nep413(signer.account_id(), &params).await?;
//!
//! // Verify the signature
//! let near = Near::testnet().build();
//! let is_valid = nep413::verify(&signed, &params, &near, Default::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Nonces
//!
//! NEP-413 defines the nonce as an arbitrary, opaque 32-byte value — the spec attaches no
//! semantics to it. As a convenience, near-kit's [`generate_nonce()`] embeds a big-endian
//! millisecond timestamp in the first 8 bytes so that verification can enforce a maximum
//! signature age. This timestamp embedding is a near-kit convention, not part of the spec.
//!
//! By default, [`verify_signature()`] and `verify()` assume this convention
//! ([`NonceValidation::Timestamp`]). If you are verifying messages from an app that uses its
//! own nonce scheme (e.g. random bytes, or a custom structured nonce), pass
//! [`NonceValidation::None`] to skip the timestamp interpretation — you are then responsible
//! for replay protection yourself (e.g. tracking used nonces or validating your own embedded
//! structure).

#[doc(inline)]
pub use near_kit_primitives::standards::nep413::{
    AuthPayload, DEFAULT_MAX_AGE, NonceValidation, SignMessageParams, SignedMessage, VerifyOptions,
    extract_timestamp_from_nonce, generate_nonce, hash_message, verify_signature,
};

#[cfg(feature = "rpc")]
use crate::Near;
#[cfg(feature = "rpc")]
use crate::error::Error;
#[cfg(feature = "rpc")]
use crate::types::{AccessKeyPermissionView, BlockReference};

/// Verify a NEP-413 signed message with full verification.
///
/// This checks:
/// - The signature is valid for the message
/// - The nonce passes validation (by default, the near-kit timestamp convention with a
///   5 minute max age — see [`NonceValidation`])
/// - The public key belongs to the claimed account (via RPC)
/// - The key has full access permission (not a function call key)
///
/// `params` must be the parameters that were signed.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::Near;
/// use near_kit::standards::nep413;
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// # let signed = todo!();
/// # let params = todo!();
/// let is_valid = nep413::verify(&signed, &params, &near, Default::default()).await?;
/// # Ok(())
/// # }
/// ```
///
/// For messages with their own nonce scheme (see [Nonces](crate::standards::nep413#nonces)),
/// disable nonce interpretation and handle replay protection yourself:
///
/// ```rust,no_run
/// use near_kit::Near;
/// use near_kit::standards::nep413::{self, NonceValidation, VerifyOptions};
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// # let signed = todo!();
/// # let params = todo!();
/// let options = VerifyOptions {
///     nonce_validation: NonceValidation::None,
///     ..Default::default()
/// };
/// let is_valid = nep413::verify(&signed, &params, &near, options).await?;
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "rpc")]
pub async fn verify(
    signed: &SignedMessage,
    params: &SignMessageParams,
    near: &Near,
    options: VerifyOptions,
) -> Result<bool, Error> {
    if !verify_signature(signed, params, options.nonce_validation) {
        return Ok(false);
    }
    if !options.require_full_access {
        return Ok(true);
    }
    // A missing key or an RPC error fails verification.
    Ok(near
        .rpc()
        .view_access_key(
            &signed.account_id,
            &signed.public_key,
            BlockReference::Finality(crate::types::Finality::Optimistic),
        )
        .await
        .is_ok_and(|key| matches!(key.permission, AccessKeyPermissionView::FullAccess)))
}

#[cfg(all(test, feature = "rpc"))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::account;
    use crate::client::test_rpc::{rpc_error, rpc_result};
    use crate::client::{BoxFuture, RpcTransport, TransportResponse};
    use crate::rpc::RpcError;
    use crate::types::{InMemorySigner, SecretKey, Signer};

    /// Answers every `EXPERIMENTAL_view_access_key` with `body` and counts calls.
    struct AccessKeyTransport {
        body: Vec<u8>,
        calls: AtomicUsize,
    }

    impl RpcTransport for AccessKeyTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], "EXPERIMENTAL_view_access_key");
            assert_eq!(request["params"]["finality"], "optimistic");
            self.calls.fetch_add(1, Ordering::Relaxed);
            let body = self.body.clone();
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    fn access_key(permission: serde_json::Value) -> Vec<u8> {
        rpc_result(serde_json::json!({
            "nonce": 1u64,
            "permission": permission,
            "block_height": 1u64,
            "block_hash": "11111111111111111111111111111111",
        }))
    }

    async fn signed_message() -> (SignedMessage, SignMessageParams) {
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: None,
        };
        let signed = signer
            .key()
            .sign_nep413(signer.account_id(), &params)
            .await
            .unwrap();
        (signed, params)
    }

    /// `verify` with the key view answered by `body`, and the number of views.
    async fn verify_with(
        signed: &SignedMessage,
        params: &SignMessageParams,
        body: Vec<u8>,
        options: VerifyOptions,
    ) -> (bool, usize) {
        let transport = Arc::new(AccessKeyTransport {
            body,
            calls: AtomicUsize::new(0),
        });
        let near = Near::custom("https://example.com", "test")
            .transport(transport.clone())
            .build();
        let valid = verify(signed, params, &near, options).await.unwrap();
        (valid, transport.calls.load(Ordering::Relaxed))
    }

    #[tokio::test]
    async fn verify_needs_a_valid_signature_and_a_full_access_key() {
        let (signed, params) = signed_message().await;
        let full_access = || access_key(serde_json::json!("FullAccess"));
        let function_call = access_key(serde_json::json!({
            "FunctionCall": {
                "allowance": null,
                "receiver_id": "myapp.com",
                "method_names": []
            }
        }));
        let missing = rpc_error(
            Some("UNKNOWN_ACCESS_KEY"),
            serde_json::json!({ "public_key": signed.public_key.to_string() }),
            serde_json::json!("access key does not exist"),
        );
        let default = VerifyOptions::default;
        let offline = || VerifyOptions {
            require_full_access: false,
            ..default()
        };

        assert_eq!(
            verify_with(&signed, &params, full_access(), default()).await,
            (true, 1)
        );
        assert_eq!(
            verify_with(&signed, &params, function_call, default()).await,
            (false, 1)
        );
        assert_eq!(
            verify_with(&signed, &params, missing, default()).await,
            (false, 1)
        );

        // Without the key check, a valid signature is enough.
        assert_eq!(
            verify_with(&signed, &params, full_access(), offline()).await,
            (true, 0)
        );

        // A signature over other params fails before any RPC call.
        let tampered = SignMessageParams {
            message: "Something else".to_string(),
            ..params.clone()
        };
        for options in [default(), offline()] {
            assert_eq!(
                verify_with(&signed, &tampered, full_access(), options).await,
                (false, 0)
            );
        }
    }
}
