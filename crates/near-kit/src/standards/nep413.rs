//! NEP-413: Off-chain message signing for authentication.
//!
//! The message types and the offline [`verify_signature()`] are re-exported
//! from `near-kit-primitives`; `verify()` (feature `rpc`) additionally checks key ownership
//! over RPC.
//!
//! NEP-413 enables users to sign messages for authentication and ownership verification
//! without gas fees or blockchain transactions.
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
//!
//! @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>

#[doc(inline)]
pub use near_kit_primitives::standards::nep413::{
    AuthPayload, DEFAULT_MAX_AGE, NEP413_TAG, NonceValidation, SignMessageParams, SignedMessage,
    VerifyOptions, extract_timestamp_from_nonce, generate_nonce, hash_message, verify_signature,
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
/// # Arguments
///
/// * `signed` - The signed message to verify
/// * `params` - Original message parameters (must match what was signed)
/// * `near` - Near client for RPC verification
/// * `options` - Verification options
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
/// For messages that use a custom nonce scheme (NEP-413 nonces are arbitrary 32 bytes;
/// the timestamp layout is a near-kit convention), disable nonce interpretation and
/// handle replay protection yourself:
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
