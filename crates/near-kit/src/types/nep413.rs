//! NEP-413: Off-chain message signing for authentication.
//!
//! NEP-413 enables users to sign messages for authentication and ownership verification
//! without gas fees or blockchain transactions.
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::{Near, InMemorySigner, nep413};
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let signer = InMemorySigner::new(
//!     "alice.testnet",
//!     "ed25519:..."
//! )?;
//!
//! let near = Near::testnet()
//!     .signer(signer)
//!     .build();
//!
//! // Sign a message
//! let params = nep413::SignMessageParams {
//!     message: "Login to MyApp".to_string(),
//!     recipient: "myapp.com".to_string(),
//!     nonce: nep413::generate_nonce(),
//!     callback_url: None,
//!     state: None,
//! };
//!
//! let signed = near.sign_message(params.clone()).await?;
//!
//! // Verify the signature
//! let is_valid = nep413::verify(&signed, &params, &near, Default::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>

use std::time::Duration;

use borsh::BorshSerialize;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{hex::Hex, serde_as};

use crate::error::Error;
use crate::types::{AccountId, BlockReference, CryptoHash, PublicKey, Signature};
use crate::Near;

/// NEP-413 tag prefix: 2^31 + 413 = 2147484061
///
/// This prefix ensures that signed messages cannot be confused with valid transactions.
/// The tag makes the message too long to be a valid signer account ID.
pub const NEP413_TAG: u32 = (1 << 31) + 413;

/// Default maximum age for signature validity (5 minutes).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(5 * 60);

// ============================================================================
// Types
// ============================================================================

/// Parameters for signing a NEP-413 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignMessageParams {
    /// The message to sign.
    pub message: String,

    /// The recipient identifier (e.g., "alice.near" or "myapp.com").
    pub recipient: String,

    /// A 32-byte nonce for replay protection.
    /// Use [`generate_nonce()`] to create one with an embedded timestamp.
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
/// ```rust,no_run
/// use near_kit::nep413::{AuthPayload, verify_signature, DEFAULT_MAX_AGE};
///
/// // Parse JSON from HTTP request body (in a real app, from req.body)
/// fn handle_login(body: &str) -> bool {
///     let payload: AuthPayload = serde_json::from_str(body).unwrap();
///     let params = payload.to_params();
///     verify_signature(&payload.signed_message, &params, DEFAULT_MAX_AGE)
/// }
/// ```
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

    /// Create an `AuthPayload` from a signed message and the original parameters.
    ///
    /// This is useful when you want to generate the same JSON format that a
    /// TypeScript client would produce, for example when testing or when a
    /// Rust client needs to authenticate to a service.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::{Near, nep413};
    ///
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// let params = nep413::SignMessageParams {
    ///     message: "Sign in to My App".to_string(),
    ///     recipient: "myapp.com".to_string(),
    ///     nonce: nep413::generate_nonce(),
    ///     callback_url: None,
    ///     state: None,
    /// };
    ///
    /// let signed = near.sign_message(params.clone()).await?;
    /// let payload = nep413::AuthPayload::from_signed(signed, &params);
    ///
    /// // Serialize to JSON for HTTP request
    /// let json = serde_json::to_string(&payload)?;
    /// println!("POST body: {}", json);
    /// # Ok(())
    /// # }
    /// ```
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

/// Internal Borsh-serializable payload matching NEP-413 spec.
#[derive(BorshSerialize)]
struct Nep413Payload {
    message: String,
    nonce: [u8; 32],
    recipient: String,
    callback_url: Option<String>,
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

/// Options for signature verification.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// Maximum age for the signature to be considered valid.
    /// Set to `Duration::MAX` to disable expiration checking.
    /// Default: 5 minutes.
    pub max_age: Duration,

    /// Whether to verify that the public key belongs to the account
    /// and has full access permission via RPC.
    /// Default: true.
    pub require_full_access: bool,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            max_age: DEFAULT_MAX_AGE,
            require_full_access: true,
        }
    }
}

// ============================================================================
// Core Functions
// ============================================================================

/// Generate a 32-byte nonce with an embedded timestamp for expiration checking.
///
/// The nonce structure:
/// - First 8 bytes: timestamp (milliseconds since epoch, big-endian)
/// - Remaining 24 bytes: cryptographically random data
///
/// # Example
///
/// ```rust
/// use near_kit::nep413;
///
/// let nonce = nep413::generate_nonce();
/// assert_eq!(nonce.len(), 32);
/// ```
pub fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];

    // First 8 bytes: timestamp (ms since epoch) as big-endian u64
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    nonce[..8].copy_from_slice(&timestamp.to_be_bytes());

    // Remaining 24 bytes: random data
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce[8..]);

    nonce
}

/// Extract the timestamp from a nonce (first 8 bytes as big-endian u64 milliseconds).
pub fn extract_timestamp_from_nonce(nonce: &[u8; 32]) -> u64 {
    u64::from_be_bytes(nonce[..8].try_into().unwrap())
}

/// Serialize and hash a NEP-413 message, ready for signing.
///
/// Steps:
/// 1. Serialize the tag (2147484061) as u32 little-endian
/// 2. Serialize the payload with Borsh
/// 3. Concatenate: tag_bytes + payload_bytes
/// 4. Hash with SHA256
///
/// # Example
///
/// ```rust
/// use near_kit::nep413::{self, SignMessageParams};
///
/// let params = SignMessageParams {
///     message: "Hello".to_string(),
///     recipient: "myapp.com".to_string(),
///     nonce: nep413::generate_nonce(),
///     callback_url: None,
///     state: None,
/// };
///
/// let hash = nep413::serialize_message(&params);
/// ```
pub fn serialize_message(params: &SignMessageParams) -> CryptoHash {
    // Serialize tag as u32 little-endian (Borsh uses little-endian)
    let tag_bytes = NEP413_TAG.to_le_bytes();

    // Serialize payload
    let payload = Nep413Payload {
        message: params.message.clone(),
        nonce: params.nonce,
        recipient: params.recipient.clone(),
        callback_url: params.callback_url.clone(),
    };
    let payload_bytes = borsh::to_vec(&payload).expect("Borsh serialization should not fail");

    // Concatenate tag + payload
    let mut combined = Vec::with_capacity(tag_bytes.len() + payload_bytes.len());
    combined.extend_from_slice(&tag_bytes);
    combined.extend_from_slice(&payload_bytes);

    // Hash with SHA256
    CryptoHash::hash(&combined)
}

/// Verify a NEP-413 signature without RPC (cryptographic verification only).
///
/// This checks:
/// - The signature is valid for the message
/// - The signature is not expired (based on nonce timestamp)
///
/// Does NOT check:
/// - Whether the public key belongs to the claimed account
/// - Whether the key has full access permission
///
/// Use [`verify()`] for full verification including RPC checks.
pub fn verify_signature(
    signed: &SignedMessage,
    params: &SignMessageParams,
    max_age: Duration,
) -> bool {
    // Check timestamp expiration if max_age is not infinite
    if max_age != Duration::MAX {
        let timestamp_ms = extract_timestamp_from_nonce(&params.nonce);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        let age_ms = now_ms.saturating_sub(timestamp_ms);

        // Check if expired or timestamp is in the future (clock skew/tampering)
        if age_ms > max_age.as_millis() as u64 || timestamp_ms > now_ms {
            return false;
        }
    }

    // Reconstruct the hash
    let hash = serialize_message(params);

    // Verify the signature
    signed.signature.verify(hash.as_bytes(), &signed.public_key)
}

/// Verify a NEP-413 signed message with full verification.
///
/// This checks:
/// - The signature is valid for the message
/// - The signature is not expired (based on nonce timestamp)
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
/// use near_kit::{Near, nep413};
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
pub async fn verify(
    signed: &SignedMessage,
    params: &SignMessageParams,
    near: &Near,
    options: VerifyOptions,
) -> Result<bool, Error> {
    // First, do cryptographic verification
    if !verify_signature(signed, params, options.max_age) {
        return Ok(false);
    }

    // If RPC verification is requested, check key ownership
    if options.require_full_access {
        // Query the access key
        let access_key_result = near
            .rpc()
            .view_access_key(
                &signed.account_id,
                &signed.public_key,
                BlockReference::optimistic(),
            )
            .await;

        match access_key_result {
            Ok(access_key) => {
                // Check if it's a full access key
                if !matches!(
                    access_key.permission,
                    crate::types::AccessKeyPermissionView::FullAccess
                ) {
                    return Ok(false);
                }
            }
            Err(_) => {
                // Key not found or RPC error - verification fails
                return Ok(false);
            }
        }
    }

    Ok(true)
}

// ============================================================================
// Serde Helpers
// ============================================================================

/// Serialize a Signature as base64 (NEP-413 spec requirement).
fn serialize_signature_base64<S>(signature: &Signature, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use base64::prelude::*;
    let base64_str = BASE64_STANDARD.encode(signature.as_bytes());
    serializer.serialize_str(&base64_str)
}

/// Deserialize a Signature from multiple formats for backwards compatibility:
/// - Base64 (NEP-413 spec)
/// - Prefixed base58 (ed25519:... or secp256k1:...)
/// - Plain base58
fn deserialize_signature_flexible<'de, D>(deserializer: D) -> Result<Signature, D::Error>
where
    D: Deserializer<'de>,
{
    use base64::prelude::*;
    use serde::de::Error;

    let s: String = String::deserialize(deserializer)?;

    // Try base64 first (NEP-413 spec)
    if let Ok(bytes) = BASE64_STANDARD.decode(&s) {
        if bytes.len() == 64 {
            return Ok(Signature::ed25519_from_bytes(
                bytes
                    .try_into()
                    .map_err(|_| D::Error::custom("Invalid signature length"))?,
            ));
        }
    }

    // Try prefixed format (ed25519:base58 or secp256k1:base58)
    if let Some(data) = s.strip_prefix("ed25519:") {
        let bytes = bs58::decode(data)
            .into_vec()
            .map_err(|e| D::Error::custom(format!("Invalid base58: {}", e)))?;
        if bytes.len() == 64 {
            return Ok(Signature::ed25519_from_bytes(
                bytes
                    .try_into()
                    .map_err(|_| D::Error::custom("Invalid signature length"))?,
            ));
        }
    }

    // Try plain base58
    if let Ok(bytes) = bs58::decode(&s).into_vec() {
        if bytes.len() == 64 {
            return Ok(Signature::ed25519_from_bytes(
                bytes
                    .try_into()
                    .map_err(|_| D::Error::custom("Invalid signature length"))?,
            ));
        }
    }

    Err(D::Error::custom(
        "Invalid signature format. Expected base64, ed25519:base58, or plain base58",
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_nonce() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        assert_eq!(nonce1.len(), 32);
        assert_eq!(nonce2.len(), 32);

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
    fn test_serialize_message() {
        let params = SignMessageParams {
            message: "Hello NEAR!".to_string(),
            recipient: "example.near".to_string(),
            nonce: [0u8; 32],
            callback_url: None,
            state: None,
        };

        let hash = serialize_message(&params);

        // Should produce a valid 32-byte hash
        assert_eq!(hash.as_bytes().len(), 32);

        // Same input should produce same hash
        let hash2 = serialize_message(&params);
        assert_eq!(hash, hash2);

        // Different input should produce different hash
        let params2 = SignMessageParams {
            message: "Hello NEAR!".to_string(),
            recipient: "other.near".to_string(),
            nonce: [0u8; 32],
            callback_url: None,
            state: None,
        };
        let hash3 = serialize_message(&params2);
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_signed_message_json_roundtrip() {
        use crate::types::SecretKey;

        let secret = SecretKey::generate_ed25519();
        let params = SignMessageParams {
            message: "Test".to_string(),
            recipient: "app.near".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: Some("csrf_token".to_string()),
        };

        let hash = serialize_message(&params);
        let signature = secret.sign(hash.as_bytes());

        let signed = SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature,
            state: params.state.clone(),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&signed).unwrap();

        // The signature should be base64, not ed25519:base58 format
        // Note: public_key still uses ed25519: prefix (that's correct per NEAR format)
        // We check that the signature field specifically is base64 by parsing
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let sig_str = json_value["signature"].as_str().unwrap();
        // Base64 signatures should not have a colon (ed25519: prefix would have one)
        assert!(
            !sig_str.contains(':'),
            "Signature should be base64, not prefixed format: {}",
            sig_str
        );

        // Deserialize back
        let deserialized: SignedMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(signed.account_id, deserialized.account_id);
        assert_eq!(signed.public_key, deserialized.public_key);
        assert_eq!(
            signed.signature.as_bytes(),
            deserialized.signature.as_bytes()
        );
        assert_eq!(signed.state, deserialized.state);
    }

    #[test]
    fn test_verify_signature_basic() {
        use crate::types::SecretKey;

        let secret = SecretKey::generate_ed25519();
        let params = SignMessageParams {
            message: "Test message".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: None,
        };

        let hash = serialize_message(&params);
        let signature = secret.sign(hash.as_bytes());

        let signed = SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature,
            state: None,
        };

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
        use crate::types::SecretKey;

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

        let hash = serialize_message(&params);
        let signature = secret.sign(hash.as_bytes());

        let signed = SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature,
            state: None,
        };

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

    /// Test interoperability with TypeScript near-kit implementation.
    /// These test vectors were verified against real wallet implementations
    /// (Meteor Wallet, MyNearWallet) in near-api-rs.
    #[test]
    fn test_typescript_interoperability() {
        use base64::prelude::*;

        // Test vector from near-api-rs, verified against Meteor wallet
        // Seed phrase: "fatal edge jacket cash hard pass gallery fabric whisper size rain biology"
        // HD path: m/44'/397'/0'
        // This produces public key: ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy

        let nonce_base64 = "KNV0cOpvJ50D5vfF9pqWom8wo2sliQ4W+Wa7uZ3Uk6Y=";
        let nonce_bytes = BASE64_STANDARD.decode(nonce_base64).unwrap();
        let nonce: [u8; 32] = nonce_bytes.try_into().unwrap();

        // Test WITHOUT callback_url (Meteor wallet style)
        let params_no_callback = SignMessageParams {
            message: "Hello NEAR!".to_string(),
            recipient: "example.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let expected_sig_no_callback =
            "NnJgPU1Ql7ccRTITIoOVsIfElmvH1RV7QAT4a9Vh6ShCOnjIzRwxqX54JzoQ/nK02p7VBMI2vJn48rpImIJwAw==";

        // Verify our serialization produces the same hash that the TS impl would sign
        let hash = serialize_message(&params_no_callback);

        // The public key from the seed phrase (derived with m/44'/397'/0')
        let public_key: crate::types::PublicKey =
            "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy"
                .parse()
                .unwrap();

        // Decode the expected signature
        let sig_bytes = BASE64_STANDARD.decode(expected_sig_no_callback).unwrap();
        let signature = crate::types::Signature::ed25519_from_bytes(
            sig_bytes.try_into().expect("signature should be 64 bytes"),
        );

        // Verify the signature is valid for our computed hash
        assert!(
            signature.verify(hash.as_bytes(), &public_key),
            "Signature verification failed - serialization mismatch with TypeScript"
        );

        // Test WITH callback_url (MyNearWallet style)
        let params_with_callback = SignMessageParams {
            message: "Hello NEAR!".to_string(),
            recipient: "example.near".to_string(),
            nonce,
            callback_url: Some("http://localhost:3000".to_string()),
            state: None,
        };

        let expected_sig_with_callback =
            "zzZQ/GwAjrZVrTIFlvmmQbDQHllfzrr8urVWHaRt5cPfcXaCSZo35c5LDpPpTKivR6BxLyb3lcPM0FfCW5lcBQ==";

        let hash = serialize_message(&params_with_callback);
        let sig_bytes = BASE64_STANDARD.decode(expected_sig_with_callback).unwrap();
        let signature = crate::types::Signature::ed25519_from_bytes(
            sig_bytes.try_into().expect("signature should be 64 bytes"),
        );

        assert!(
            signature.verify(hash.as_bytes(), &public_key),
            "Signature verification with callback_url failed - serialization mismatch"
        );
    }

    /// Test that we can deserialize a SignedMessage JSON from TypeScript.
    #[test]
    fn test_deserialize_typescript_signed_message() {
        // Simulated JSON from TypeScript near-kit
        // Using the correct public key derived from seed phrase
        let ts_json = r#"{
            "accountId": "alice.testnet",
            "publicKey": "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy",
            "signature": "NnJgPU1Ql7ccRTITIoOVsIfElmvH1RV7QAT4a9Vh6ShCOnjIzRwxqX54JzoQ/nK02p7VBMI2vJn48rpImIJwAw=="
        }"#;

        // Should deserialize successfully
        let signed: SignedMessage = serde_json::from_str(ts_json).unwrap();

        assert_eq!(signed.account_id.as_str(), "alice.testnet");
        assert_eq!(
            signed.public_key.to_string(),
            "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy"
        );
        assert_eq!(signed.signature.as_bytes().len(), 64);
        assert!(signed.state.is_none());
    }

    /// Test that we can deserialize legacy ed25519:base58 signature format.
    #[test]
    fn test_deserialize_legacy_base58_signature() {
        // Some older implementations might send signatures in ed25519:base58 format
        let legacy_json = r#"{
            "accountId": "alice.testnet",
            "publicKey": "ed25519:HeEp8gQPzs6rMPRN1hijJ7dXFmZLu3FPNKeLDpmLfFBT",
            "signature": "ed25519:2DzVcjvceXbR6n9ot4C9xA8gVPrZRq8NqJj4b3DaLBmVk1TqXwK8yHcL6M6ezQD4HxXHhZQPbgjdNW7Tx8sjxSFe"
        }"#;

        // Should deserialize successfully (backwards compatibility)
        let signed: SignedMessage = serde_json::from_str(legacy_json).unwrap();

        assert_eq!(signed.account_id.as_str(), "alice.testnet");
        assert_eq!(signed.signature.as_bytes().len(), 64);
    }

    /// Test roundtrip: Rust -> JSON -> TypeScript-compatible -> JSON -> Rust
    #[test]
    fn test_rust_to_typescript_roundtrip() {
        use crate::types::SecretKey;

        let secret = SecretKey::generate_ed25519();
        let params = SignMessageParams {
            message: "Cross-platform test".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: Some("session123".to_string()),
        };

        let hash = serialize_message(&params);
        let signature = secret.sign(hash.as_bytes());

        let signed = SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature,
            state: params.state.clone(),
        };

        // Serialize to JSON (as if sending to TypeScript)
        let json = serde_json::to_string(&signed).unwrap();

        // Parse as generic JSON to inspect the format
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Verify field names are camelCase (TypeScript compatible)
        assert!(json_value.get("accountId").is_some());
        assert!(json_value.get("publicKey").is_some());
        assert!(json_value.get("signature").is_some());

        // Verify signature is base64 (no colon = not prefixed format)
        let sig_str = json_value["signature"].as_str().unwrap();
        assert!(!sig_str.contains(':'));

        // Deserialize back (as if receiving from TypeScript)
        let roundtrip: SignedMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(signed.account_id, roundtrip.account_id);
        assert_eq!(signed.public_key, roundtrip.public_key);
        assert_eq!(signed.signature.as_bytes(), roundtrip.signature.as_bytes());
        assert_eq!(signed.state, roundtrip.state);

        // Verify signature still works after roundtrip
        assert!(verify_signature(&roundtrip, &params, Duration::MAX));
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

    /// Test the complete authentication flow: TS client -> JSON -> Rust server
    #[test]
    fn test_full_auth_flow_interop() {
        // Simulate what a real HTTP request body would look like from near-kit TS
        // Nonce is hex encoded (64 chars for 32 bytes)
        let http_body = r#"{
            "signedMessage": {
                "accountId": "alice.testnet",
                "publicKey": "ed25519:2RM3EotCzEiVobm6aMjaup43k8cFffR4KHFtrqbZ79Qy",
                "signature": "NnJgPU1Ql7ccRTITIoOVsIfElmvH1RV7QAT4a9Vh6ShCOnjIzRwxqX54JzoQ/nK02p7VBMI2vJn48rpImIJwAw=="
            },
            "nonce": "28d57470ea6f279d03e6f7c5f69a96a26f30a36b25890e16f966bbb99dd493a6",
            "message": "Hello NEAR!",
            "recipient": "example.near"
        }"#;

        // Parse as AuthPayload
        let payload: AuthPayload = serde_json::from_str(http_body).unwrap();

        // Verify the signature
        let params = payload.to_params();
        let is_valid = verify_signature(&payload.signed_message, &params, Duration::MAX);

        assert!(is_valid, "Signature should be valid");
        assert_eq!(payload.signed_message.account_id.as_str(), "alice.testnet");
    }

    /// Test generating AuthPayload from Rust and serializing to JSON
    #[test]
    fn test_generate_auth_payload_from_rust() {
        use crate::types::SecretKey;

        let secret = SecretKey::generate_ed25519();
        let params = SignMessageParams {
            message: "Sign in to My App".to_string(),
            recipient: "myapp.com".to_string(),
            nonce: generate_nonce(),
            callback_url: None,
            state: None,
        };

        let hash = serialize_message(&params);
        let signature = secret.sign(hash.as_bytes());

        let signed = SignedMessage {
            account_id: "alice.near".parse().unwrap(),
            public_key: secret.public_key(),
            signature,
            state: None,
        };

        // Create AuthPayload (what would be sent over HTTP)
        let payload = AuthPayload::from_signed(signed.clone(), &params);

        // Serialize to JSON
        let json = serde_json::to_string(&payload).unwrap();

        // Verify nonce is hex (not an array)
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let nonce_str = json_value["nonce"].as_str().unwrap();
        assert!(
            nonce_str.len() == 64, // Hex of 32 bytes = 64 chars
            "Nonce should be hex encoded, got: {}",
            nonce_str
        );

        // Deserialize back and verify
        let roundtrip: AuthPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload.nonce, roundtrip.nonce);
        assert_eq!(payload.message, roundtrip.message);

        // Verify signature still works
        let roundtrip_params = roundtrip.to_params();
        assert!(verify_signature(
            &roundtrip.signed_message,
            &roundtrip_params,
            Duration::MAX
        ));
    }
}
