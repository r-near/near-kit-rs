//! NEP-413: Off-chain message signing for authentication.
//!
//! NEP-413 enables users to sign messages for authentication and ownership verification
//! without gas fees or blockchain transactions.
//!
//! # Example
//!
//! ```rust
//! use near_kit_primitives::signer::{InMemorySigner, SecretKey, Signer};
//! use near_kit_primitives::standards::nep413;
//!
//! # async fn example() -> Result<(), near_kit_primitives::signer::SignerError> {
//! let signer = InMemorySigner::implicit(SecretKey::generate_ed25519())?;
//!
//! // Sign a message
//! let params = nep413::SignMessageParams {
//!     message: "Login to MyApp".to_string(),
//!     recipient: "myapp.com".to_string(),
//!     nonce: nep413::generate_nonce(),
//!     callback_url: None,
//!     state: None,
//! };
//! let signed = signer.key().sign_nep413(signer.account_id(), &params).await?;
//!
//! // Verify the signature (offline: checks the signature and nonce age only)
//! assert!(nep413::verify_signature(&signed, &params, nep413::NonceValidation::default()));
//! # Ok(())
//! # }
//! ```
//!
//! [`verify_signature()`] does not check that the public key belongs to the
//! claimed account. `near-kit`'s `nep413::verify` adds that check by querying
//! the account's access keys over RPC.
//!
//! # Nonces
//!
//! NEP-413 defines the nonce as an arbitrary, opaque 32-byte value; the spec attaches no
//! semantics to it. As a convenience, near-kit's [`generate_nonce()`] embeds a big-endian
//! millisecond timestamp in the first 8 bytes so that verification can enforce a maximum
//! signature age. This timestamp embedding is a near-kit convention, not part of the spec.
//!
//! By default, [`verify_signature()`] (and `near-kit`'s RPC-backed `verify()`) assume this
//! convention ([`NonceValidation::Timestamp`]). If you are verifying messages from an app that
//! uses its own nonce scheme (e.g. random bytes, or a custom structured nonce), pass
//! [`NonceValidation::None`] to skip the timestamp interpretation. You are then responsible
//! for replay protection yourself (e.g. tracking used nonces or validating your own embedded
//! structure).
//!
//! @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>

use std::time::Duration;

use base64::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{hex::Hex, serde_as};

use crate::types::{AccountId, CryptoHash, PublicKey, Signature};

/// NEP-413 tag prefix.
///
/// This prefix ensures that signed messages cannot be confused with valid transactions.
/// The tag makes the message too long to be a valid signer account ID.
pub(crate) const NEP413_TAG: u32 = (1 << 31) + 413;

/// Default maximum age for signature validity (5 minutes).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// Parameters for signing a NEP-413 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignMessageParams {
    /// The message to sign.
    pub message: String,

    /// The recipient identifier (e.g., "alice.near" or "myapp.com").
    pub recipient: String,

    /// A 32-byte nonce for replay protection; see [Nonces](self#nonces).
    pub nonce: [u8; 32],

    /// Optional callback URL for web wallets.
    pub callback_url: Option<String>,

    /// Optional state parameter for CSRF protection.
    pub state: Option<String>,
}

/// HTTP request payload for NEP-413 authentication.
///
/// This is the typical JSON structure sent from a frontend to a backend
/// for authentication. Use this to deserialize the HTTP request body.
///
/// # Example
///
/// The recipient, message and nonce come from the request, so a login
/// handler compares them with what the app asked for: otherwise a signature
/// the user made for another app, or a replay of an earlier login, passes.
///
/// ```rust,no_run
/// use std::collections::HashSet;
///
/// use near_kit_primitives::standards::nep413::{AuthPayload, NonceValidation, verify_signature};
///
/// /// The offline checks on a login request body (in a real app, from
/// /// req.body). `issued` holds the nonces this server generated with
/// /// `generate_nonce()` and handed to the frontend.
/// fn check_login(body: &str, issued: &HashSet<[u8; 32]>) -> Option<AuthPayload> {
///     let payload: AuthPayload = serde_json::from_str(body).ok()?;
///     if payload.recipient != "example.app"
///         || payload.message != "Log in to example.app"
///         || !issued.contains(&payload.nonce)
///     {
///         return None;
///     }
///     let params = payload.to_params();
///     verify_signature(&payload.signed_message, &params, NonceValidation::default())
///         .then_some(payload)
/// }
/// ```
///
/// `verify_signature` checks the signature and the nonce's age, not that the
/// key belongs to the account the message names. Before accepting the
/// account, confirm that with near-kit's `nep413::verify` (an RPC lookup),
/// then remove the nonce from `issued` so it is accepted once.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPayload {
    /// The signed message from the client.
    pub signed_message: SignedMessage,

    /// The nonce as a hex-encoded string (64 characters for 32 bytes).
    #[serde_as(as = "Hex")]
    pub nonce: [u8; 32],

    /// The message that was signed (must match what the client signed).
    pub message: String,

    /// The recipient identifier (must match what the client signed).
    pub recipient: String,

    /// Optional callback URL (must match what the client signed, if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

impl AuthPayload {
    /// Convert to [`SignMessageParams`] for verification.
    pub fn to_params(&self) -> SignMessageParams {
        SignMessageParams {
            message: self.message.clone(),
            recipient: self.recipient.clone(),
            nonce: self.nonce,
            callback_url: self.callback_url.clone(),
            state: self.signed_message.state.clone(),
        }
    }

    /// The payload a client sends for `signed_message`, signed over `params`.
    pub fn from_signed(signed_message: SignedMessage, params: &SignMessageParams) -> Self {
        Self {
            signed_message,
            nonce: params.nonce,
            message: params.message.clone(),
            recipient: params.recipient.clone(),
            callback_url: params.callback_url.clone(),
        }
    }
}

/// A signed NEP-413 message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedMessage {
    /// The account that signed the message.
    pub account_id: AccountId,

    /// The public key used to sign.
    pub public_key: PublicKey,

    /// The signature (base64 encoded in JSON).
    #[serde(
        serialize_with = "serialize_signature_base64",
        deserialize_with = "deserialize_signature_flexible"
    )]
    pub signature: Signature,

    /// Optional state parameter for CSRF protection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// How to interpret the nonce during verification; see [Nonces](self#nonces).
///
/// A [`Duration`] converts into this type for convenience: `max_age.into()` yields
/// `Timestamp { max_age }`, and `Duration::MAX` yields `None` (no expiration checking).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceValidation {
    /// Interpret the first 8 bytes of the nonce as a big-endian millisecond timestamp
    /// (the near-kit convention produced by [`generate_nonce()`]) and reject signatures
    /// older than `max_age` or with a timestamp in the future.
    Timestamp {
        /// Maximum age for the signature to be considered valid.
        max_age: Duration,
    },

    /// Treat the nonce as opaque, per the NEP-413 spec; replay protection is then
    /// the caller's job.
    None,
}

impl Default for NonceValidation {
    /// Defaults to [`NonceValidation::Timestamp`] with [`DEFAULT_MAX_AGE`] (5 minutes).
    fn default() -> Self {
        Self::Timestamp {
            max_age: DEFAULT_MAX_AGE,
        }
    }
}

impl From<Duration> for NonceValidation {
    /// Maps a maximum age to [`NonceValidation::Timestamp`].
    /// `Duration::MAX` maps to [`NonceValidation::None`] (disables expiration checking).
    fn from(max_age: Duration) -> Self {
        if max_age == Duration::MAX {
            Self::None
        } else {
            Self::Timestamp { max_age }
        }
    }
}

/// Options for signature verification.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// How to interpret the nonce. Defaults to [`NonceValidation::Timestamp`] with a
    /// 5 minute maximum age (matches nonces from [`generate_nonce()`]).
    /// Use [`NonceValidation::None`] for messages that use a custom nonce scheme.
    pub nonce_validation: NonceValidation,

    /// Whether to verify that the public key belongs to the account
    /// and has full access permission via RPC.
    /// Default: true.
    pub require_full_access: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            nonce_validation: NonceValidation::default(),
            require_full_access: true,
        }
    }
}

/// Current time in milliseconds since the Unix epoch.
///
/// `std::time::SystemTime::now()` panics on `wasm32-unknown-unknown` (there's no OS
/// clock without a JS shim), so that target uses `js_sys::Date::now()` instead.
/// WASI targets have a real clock and take the `SystemTime` path.
fn now_millis() -> u64 {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        js_sys::Date::now() as u64
    }
}

/// Generate a 32-byte nonce: a big-endian millisecond timestamp, then 24 random
/// bytes (a near-kit convention; see [Nonces](self#nonces)).
pub fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    nonce[..8].copy_from_slice(&now_millis().to_be_bytes());
    super::csprng::fill_random(&mut nonce[8..]);
    nonce
}

/// Extract the timestamp from a nonce (first 8 bytes as big-endian u64 milliseconds).
/// Only meaningful for nonces from [`generate_nonce()`].
pub fn extract_timestamp_from_nonce(nonce: &[u8; 32]) -> u64 {
    u64::from_be_bytes(nonce[..8].try_into().unwrap())
}

/// Hash a NEP-413 message, ready for signing: SHA-256 of the borsh tag
/// 2^31 + 413 (u32 LE) then borsh `{message, nonce, recipient, callback_url}`.
pub fn hash_message(p: &SignMessageParams) -> CryptoHash {
    let payload = (
        NEP413_TAG,
        &p.message,
        &p.nonce,
        &p.recipient,
        &p.callback_url,
    );
    CryptoHash::hash(&borsh::to_vec(&payload).expect("borsh to Vec cannot fail"))
}

/// Verify a NEP-413 signature and the nonce without RPC.
///
/// This doesn't check that the key belongs to the account (see the
/// [module docs](self)). `nonce_validation` accepts a [`NonceValidation`] or a
/// [`Duration`] max age (`Duration::MAX` disables expiration checking).
pub fn verify_signature(
    signed: &SignedMessage,
    params: &SignMessageParams,
    nonce_validation: impl Into<NonceValidation>,
) -> bool {
    if let NonceValidation::Timestamp { max_age } = nonce_validation.into() {
        let timestamp_ms = extract_timestamp_from_nonce(&params.nonce);
        let now_ms = now_millis();

        let age_ms = now_ms.saturating_sub(timestamp_ms);

        // Check if expired or timestamp is in the future (clock skew/tampering).
        // Compare in u128 so very large max_age values don't truncate.
        if u128::from(age_ms) > max_age.as_millis() || timestamp_ms > now_ms {
            return false;
        }
    }

    let hash = hash_message(params);
    signed.signature.verify(hash.as_bytes(), &signed.public_key)
}

/// Serialize a Signature as base64 (NEP-413 spec requirement).
fn serialize_signature_base64<S>(signature: &Signature, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&BASE64_STANDARD.encode(signature.as_bytes()))
}

/// Deserialize a Signature from the formats clients send:
/// - Prefixed base58 (`ed25519:...`, `secp256k1:...` or `ml-dsa-65:...`, as
///   near-cli-rs `sign-nep413` prints it)
/// - Base64 (NEP-413 spec)
/// - Plain base58
///
/// Unprefixed payloads pick the key type from the decoded length: 64 bytes is
/// Ed25519, 65 Secp256k1 and 3309 ML-DSA-65.
fn deserialize_signature_flexible<'de, D>(deserializer: D) -> Result<Signature, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let s: String = String::deserialize(deserializer)?;

    // Base64 and base58 never contain ':', so a colon means a key-type prefix.
    if s.contains(':') {
        return s
            .parse::<Signature>()
            .map_err(|e| D::Error::custom(format!("Invalid signature: {e}")));
    }

    // Base64 first (NEP-413 spec), then plain base58.
    let decoded = [
        BASE64_STANDARD.decode(&s).ok(),
        bs58::decode(&s).into_vec().ok(),
    ];
    for bytes in decoded.into_iter().flatten() {
        if let Some(signature) = signature_from_unprefixed_bytes(&bytes) {
            return Ok(signature);
        }
    }

    Err(D::Error::custom(
        "Invalid signature format. Expected base64, <key-type>:base58, or plain base58 \
         of a 64-byte (ed25519), 65-byte (secp256k1) or 3309-byte (ml-dsa-65) signature",
    ))
}

/// Build a signature from raw bytes, choosing the key type by length.
fn signature_from_unprefixed_bytes(bytes: &[u8]) -> Option<Signature> {
    use crate::types::key::ML_DSA_65_SIGNATURE_LENGTH;

    match bytes.len() {
        64 => bytes.try_into().ok().map(Signature::Ed25519),
        65 => bytes.try_into().ok().map(Signature::Secp256k1),
        ML_DSA_65_SIGNATURE_LENGTH => bytes
            .try_into()
            .ok()
            .map(|b| Signature::MlDsa65(Box::new(b))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SecretKey;

    fn sign(secret: &SecretKey, params: &SignMessageParams) -> SignedMessage {
        SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature: secret.sign(hash_message(params).as_bytes()),
            state: params.state.clone(),
        }
    }

    #[test]
    fn test_generate_nonce() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        // Nonces should be different (random part)
        assert_ne!(nonce1, nonce2);

        // Timestamps should be recent (within 1 second)
        let ts1 = extract_timestamp_from_nonce(&nonce1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(now - ts1 < 1000);
    }

    #[test]
    fn test_verify_signature_basic() {
        let secret = SecretKey::generate_ed25519();
        let params = SignMessageParams {
            message: "Test message".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: None,
        };

        let signed = sign(&secret, &params);

        // Should verify successfully
        assert!(verify_signature(&signed, &params, DEFAULT_MAX_AGE));

        // Should fail with wrong message
        let wrong_params = SignMessageParams {
            message: "Wrong message".to_string(),
            ..params.clone()
        };
        assert!(!verify_signature(&signed, &wrong_params, DEFAULT_MAX_AGE));

        // Should fail with wrong public key
        let other_secret = SecretKey::generate_ed25519();
        let wrong_signed = SignedMessage {
            public_key: other_secret.public_key(),
            ..signed.clone()
        };
        assert!(!verify_signature(&wrong_signed, &params, DEFAULT_MAX_AGE));
    }

    #[test]
    fn test_verify_signature_expiration() {
        let secret = SecretKey::generate_ed25519();

        // Create a nonce with an old timestamp (10 minutes ago)
        let mut old_nonce = [0u8; 32];
        let old_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - (10 * 60 * 1000); // 10 minutes ago
        old_nonce[..8].copy_from_slice(&old_timestamp.to_be_bytes());

        let params = SignMessageParams {
            message: "Test".to_string(),
            recipient: "app.com".to_string(),
            nonce: old_nonce,
            callback_url: None,
            state: None,
        };

        let signed = sign(&secret, &params);

        // Should fail with default max age (5 minutes)
        assert!(!verify_signature(&signed, &params, DEFAULT_MAX_AGE));

        // Should pass with longer max age
        assert!(verify_signature(
            &signed,
            &params,
            Duration::from_secs(15 * 60)
        ));

        // Should pass with infinite max age
        assert!(verify_signature(&signed, &params, Duration::MAX));
    }

    #[test]
    fn test_verify_signature_custom_nonce_scheme() {
        let secret = SecretKey::generate_ed25519();

        // A custom nonce that does NOT follow the near-kit timestamp convention:
        // the first 8 bytes are garbage as a timestamp (far-future value).
        let mut custom_nonce = [7u8; 32];
        custom_nonce[..8].copy_from_slice(&u64::MAX.to_be_bytes());

        let params = SignMessageParams {
            message: "Custom nonce scheme".to_string(),
            recipient: "intents.near".to_string(),
            nonce: custom_nonce,
            callback_url: None,
            state: None,
        };

        let signed = sign(&secret, &params);

        // Fails with the default timestamp validation (first 8 bytes look like a
        // far-future timestamp)
        assert!(!verify_signature(
            &signed,
            &params,
            NonceValidation::default()
        ));

        // Verifies successfully when the nonce is treated as opaque
        assert!(verify_signature(&signed, &params, NonceValidation::None));

        // Still fails cryptographic verification with a tampered message
        let wrong_params = SignMessageParams {
            message: "Tampered".to_string(),
            ..params
        };
        assert!(!verify_signature(
            &signed,
            &wrong_params,
            NonceValidation::None
        ));
    }

    #[test]
    fn test_nonce_validation_from_duration() {
        // A finite duration maps to timestamp validation with that max age
        assert_eq!(
            NonceValidation::from(Duration::from_secs(60)),
            NonceValidation::Timestamp {
                max_age: Duration::from_secs(60)
            }
        );

        // Duration::MAX preserves the documented "disable expiration" behavior
        assert_eq!(NonceValidation::from(Duration::MAX), NonceValidation::None);

        // Default matches the previous default behavior
        assert_eq!(
            NonceValidation::default(),
            NonceValidation::Timestamp {
                max_age: DEFAULT_MAX_AGE
            }
        );
    }

    /// Test interoperability with TypeScript near-kit implementation.
    /// These test vectors were verified against real wallet implementations
    /// (Meteor Wallet, MyNearWallet) in near-api-rs.
    #[test]
    fn test_typescript_interoperability() {
        // Seed phrase "fatal edge jacket cash hard pass gallery fabric whisper size
        // rain biology" at m/44'/397'/0'; Meteor wallet signs without a
        // callback_url, MyNearWallet with one.
        let public_key: crate::types::PublicKey =
            "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy"
                .parse()
                .unwrap();
        let nonce = BASE64_STANDARD
            .decode("KNV0cOpvJ50D5vfF9pqWom8wo2sliQ4W+Wa7uZ3Uk6Y=")
            .unwrap()
            .try_into()
            .unwrap();
        for (callback_url, signature) in [
            (
                None,
                "NnJgPU1Ql7ccRTITIoOVsIfElmvH1RV7QAT4a9Vh6ShCOnjIzRwxqX54JzoQ/nK02p7VBMI2vJn48rpImIJwAw==",
            ),
            (
                Some("http://localhost:3000"),
                "zzZQ/GwAjrZVrTIFlvmmQbDQHllfzrr8urVWHaRt5cPfcXaCSZo35c5LDpPpTKivR6BxLyb3lcPM0FfCW5lcBQ==",
            ),
        ] {
            let params = SignMessageParams {
                message: "Hello NEAR!".to_string(),
                recipient: "example.near".to_string(),
                nonce,
                callback_url: callback_url.map(String::from),
                state: None,
            };
            let signature = crate::types::Signature::Ed25519(
                BASE64_STANDARD
                    .decode(signature)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
            assert!(
                signature.verify(hash_message(&params).as_bytes(), &public_key),
                "callback_url {callback_url:?}: hash differs from the wallets'"
            );
        }
    }

    /// Test deserializing the full HTTP authentication payload from TypeScript.
    #[test]
    fn test_deserialize_http_auth_payload() {
        // This is what a TypeScript frontend would send to the backend
        // nonce is sent as hex (64 chars for 32 bytes)
        let nonce_hex = "28d57470ea6f279d03e6f7c5f69a96a26f30a36b25890e16f966bbb99dd493a6";

        // Build the JSON as the TS client would
        let http_payload = serde_json::json!({
            "signedMessage": {
                "accountId": "alice.testnet",
                "publicKey": "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy",
                "signature": "NnJgPU1Ql7ccRTITIoOVsIfElmvH1RV7QAT4a9Vh6ShCOnjIzRwxqX54JzoQ/nK02p7VBMI2vJn48rpImIJwAw=="
            },
            "nonce": nonce_hex,
            "message": "Hello NEAR!",
            "recipient": "example.near"
        });

        // Deserialize the HTTP payload
        let payload: AuthPayload = serde_json::from_value(http_payload).unwrap();

        // Verify we got the right data
        assert_eq!(payload.signed_message.account_id.as_str(), "alice.testnet");
        assert_eq!(payload.message, "Hello NEAR!");
        assert_eq!(payload.recipient, "example.near");
        assert_eq!(payload.nonce.len(), 32);

        // Convert to params and verify the signature
        let params = payload.to_params();
        assert!(verify_signature(
            &payload.signed_message,
            &params,
            Duration::MAX
        ));
    }

    /// One key of each type, with a signed message for fixed params.
    fn signed_by_every_key_type() -> Vec<(SignedMessage, SignMessageParams)> {
        let params = SignMessageParams {
            message: "Login".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: [1; 32],
            callback_url: None,
            state: Some("csrf_token".to_string()),
        };
        let keys = [
            SecretKey::ed25519_from_bytes([7; 32]),
            SecretKey::secp256k1_from_bytes([2; 32]).unwrap(),
            SecretKey::ml_dsa65_from_seed([3; 32]),
        ];
        keys.iter()
            .map(|secret| (sign(secret, &params), params.clone()))
            .collect()
    }

    #[test]
    fn signed_message_json_roundtrips_for_every_key_type() {
        for (signed, params) in signed_by_every_key_type() {
            let key_type = signed.public_key.key_type();
            let json = serde_json::to_string(&signed).unwrap();
            let back: SignedMessage = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{key_type:?} SignedMessage round-trip: {e}"));
            assert_eq!(back, signed);
            assert!(verify_signature(&back, &params, NonceValidation::None));
        }
    }

    #[test]
    fn auth_payload_json_shape() {
        let (signed, params) = signed_by_every_key_type().remove(0);
        let payload = AuthPayload::from_signed(signed.clone(), &params);
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::json!({
                "signedMessage": {
                    "accountId": "alice.near",
                    "publicKey": signed.public_key.to_string(),
                    "signature": BASE64_STANDARD.encode(signed.signature.as_bytes()),
                    "state": "csrf_token",
                },
                "nonce": "01".repeat(32),
                "message": "Login",
                "recipient": "myapp.com",
            })
        );
    }

    #[test]
    fn auth_payload_json_roundtrips_for_every_key_type() {
        for (signed, params) in signed_by_every_key_type() {
            let key_type = signed.public_key.key_type();
            let payload = AuthPayload::from_signed(signed.clone(), &params);
            let json = serde_json::to_string(&payload).unwrap();
            let back: AuthPayload = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{key_type:?} AuthPayload round-trip: {e}"));
            assert_eq!(back.signed_message, signed);
            assert!(verify_signature(
                &back.signed_message,
                &back.to_params(),
                NonceValidation::None
            ));
        }
    }

    /// near-cli-rs `sign-nep413` prints `{accountId, publicKey, signature}`
    /// with the signature as `<key-type>:<base58>`.
    #[test]
    fn near_cli_rs_signed_message_parses_for_every_key_type() {
        for (signed, params) in signed_by_every_key_type() {
            let key_type = signed.public_key.key_type();
            let json = serde_json::json!({
                "accountId": signed.account_id.as_str(),
                "publicKey": signed.public_key.to_string(),
                "signature": signed.signature.to_string(),
            });
            let back: SignedMessage = serde_json::from_value(json)
                .unwrap_or_else(|e| panic!("{key_type:?} near-cli-rs JSON: {e}"));
            assert_eq!(
                back,
                SignedMessage {
                    state: None,
                    ..signed
                }
            );
            assert!(verify_signature(&back, &params, NonceValidation::None));
        }
    }

    #[test]
    fn unprefixed_base58_signature_parses_for_every_key_type() {
        for (signed, _) in signed_by_every_key_type() {
            let key_type = signed.public_key.key_type();
            let json = serde_json::json!({
                "accountId": signed.account_id.as_str(),
                "publicKey": signed.public_key.to_string(),
                "signature": bs58::encode(signed.signature.as_bytes()).into_string(),
            });
            let back: SignedMessage = serde_json::from_value(json)
                .unwrap_or_else(|e| panic!("{key_type:?} plain base58: {e}"));
            assert_eq!(back.signature, signed.signature);
        }
    }

    #[test]
    fn signature_of_unknown_length_is_rejected() {
        // 0xFF bytes base64-encode to '/', which is not base58, so neither
        // encoding can be misread as the other.
        for bytes in [vec![0xFF; 63], vec![0xFF; 66], vec![0xFF; 3308]] {
            for encoded in [
                BASE64_STANDARD.encode(&bytes),
                bs58::encode(&bytes).into_string(),
            ] {
                let json = serde_json::json!({
                    "accountId": "alice.near",
                    "publicKey": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847",
                    "signature": encoded,
                });
                assert!(serde_json::from_value::<SignedMessage>(json).is_err());
            }
        }
        let json = serde_json::json!({
            "accountId": "alice.near",
            "publicKey": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847",
            "signature": format!("secp256k1:{}", bs58::encode([0u8; 64]).into_string()),
        });
        assert!(serde_json::from_value::<SignedMessage>(json).is_err());
    }
}
