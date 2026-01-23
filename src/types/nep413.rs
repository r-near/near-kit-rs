//! NEP-413: Message signing utilities.
//!
//! NEP-413 enables off-chain message signing for authentication and ownership verification
//! without gas fees or blockchain transactions.
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::prelude::*;
//! use near_kit::nep413::{generate_nonce, SignMessageParams, verify_signature, VerifyError};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let near = Near::testnet()
//!         .credentials("ed25519:...", "alice.testnet")?
//!         .build();
//!
//!     let nonce = generate_nonce();
//!     let signed = near.sign_message(SignMessageParams {
//!         message: "Login to MyApp".to_string(),
//!         recipient: "myapp.com".to_string(),
//!         nonce,
//!         callback_url: None,
//!         state: None,
//!     })?;
//!
//!     // Verify on server
//!     let is_valid = verify_signature(&signed, &SignMessageParams {
//!         message: "Login to MyApp".to_string(),
//!         recipient: "myapp.com".to_string(),
//!         nonce: signed.nonce,
//!         callback_url: None,
//!         state: None,
//!     }, None)?;
//!
//!     Ok(())
//! }
//! ```
//!
//! @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use borsh::BorshSerialize;
use ed25519_dalek::VerifyingKey;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::Nep413Error;
use crate::types::{AccountId, KeyType, PublicKey, SecretKey};

/// NEP-413 tag prefix: 2^31 + 413 = 2147484061
///
/// This prefix ensures that signed messages cannot be confused with valid transactions.
/// The tag makes the message too long to be a valid signer account ID.
pub const NEP413_TAG: u32 = 2147484061;

/// Default maximum age for signature verification (5 minutes).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// Parameters for signing a message (NEP-413).
///
/// @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>
#[derive(Debug, Clone)]
pub struct SignMessageParams {
    /// The message that wants to be transmitted.
    pub message: String,
    /// The recipient to whom the message is destined (e.g., "alice.near" or "myapp.com").
    pub recipient: String,
    /// A nonce that uniquely identifies this instance of the message (32 bytes).
    pub nonce: [u8; 32],
    /// Optional callback URL for browser wallets.
    pub callback_url: Option<String>,
    /// Optional state for CSRF protection (browser wallets).
    pub state: Option<String>,
}

/// Signed message result (NEP-413).
///
/// @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>
#[derive(Debug, Clone)]
pub struct SignedMessage {
    /// The account name to which the public key corresponds.
    pub account_id: AccountId,
    /// The public key used to sign.
    pub public_key: PublicKey,
    /// The base64-encoded signature.
    pub signature: String,
    /// The nonce used for signing (for convenience in verification).
    pub nonce: [u8; 32],
    /// Optional state returned from browser wallets (for CSRF protection).
    pub state: Option<String>,
}

/// NEP-413 message payload for Borsh serialization.
///
/// Fields are serialized in this order:
/// 1. message: string - The message to sign
/// 2. nonce: [u8; 32] - 32-byte nonce for replay protection
/// 3. recipient: string - Recipient identifier (e.g., "alice.near" or "myapp.com")
/// 4. callback_url: Option<string> - Optional callback URL for web wallets
#[derive(BorshSerialize)]
struct Nep413Payload {
    message: String,
    nonce: [u8; 32],
    recipient: String,
    callback_url: Option<String>,
}

/// Serialize NEP-413 message parameters for signing.
///
/// Serialization steps:
/// 1. Serialize the tag (2147484061) as little-endian u32
/// 2. Serialize the payload (message, nonce, recipient, callback_url) as Borsh
/// 3. Concatenate: tag_bytes + payload_bytes
/// 4. Hash with SHA256
///
/// Returns the 32-byte SHA256 hash ready for signing.
pub fn serialize_message(params: &SignMessageParams) -> [u8; 32] {
    // Serialize tag as little-endian u32 (Borsh format)
    let tag_bytes = NEP413_TAG.to_le_bytes();

    // Serialize payload using Borsh
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

    // Hash the combined bytes
    let mut hasher = Sha256::new();
    hasher.update(&combined);
    hasher.finalize().into()
}

/// Generate a nonce for NEP-413 message signing.
///
/// Embeds a timestamp for automatic expiration checking.
/// Format: 8 bytes timestamp (big-endian ms since epoch) + 24 bytes random.
///
/// This matches the TypeScript implementation for interoperability.
pub fn generate_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];

    // First 8 bytes: timestamp (ms since epoch) as big-endian u64
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    nonce[..8].copy_from_slice(&timestamp.to_be_bytes());

    // Remaining 24 bytes: random data
    rand::thread_rng().fill_bytes(&mut nonce[8..]);

    nonce
}

/// Extract timestamp from a nonce.
///
/// Returns the timestamp in milliseconds since epoch, or None if the nonce
/// doesn't contain a valid timestamp.
pub fn extract_timestamp(nonce: &[u8; 32]) -> u64 {
    u64::from_be_bytes(nonce[..8].try_into().unwrap())
}

/// Options for NEP-413 signature verification.
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    /// Maximum age in milliseconds for the signature to be considered valid.
    /// Default: 5 minutes (300,000 ms).
    /// Set to `None` to disable timestamp checking.
    pub max_age: Option<Duration>,
}

/// Error during NEP-413 verification.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VerifyError {
    #[error("Signature expired: age {age_ms}ms exceeds max age {max_age_ms}ms")]
    Expired { age_ms: u64, max_age_ms: u64 },

    #[error("Signature timestamp is in the future")]
    FutureTimestamp,

    #[error("Only Ed25519 keys are supported for NEP-413")]
    UnsupportedKeyType,

    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("Invalid signature encoding")]
    InvalidSignatureEncoding,

    #[error("Signature verification failed")]
    SignatureInvalid,
}

/// Verify a NEP-413 signed message.
///
/// Automatically checks timestamp expiration (default: 5 minutes).
/// You must still track used nonces to prevent replay attacks.
///
/// # Arguments
///
/// * `signed_message` - The signed message to verify
/// * `params` - Original message parameters (must match what was signed)
/// * `options` - Verification options (optional)
///
/// # Returns
///
/// `Ok(true)` if signature is valid and not expired, `Err` otherwise.
pub fn verify_signature(
    signed_message: &SignedMessage,
    params: &SignMessageParams,
    options: Option<VerifyOptions>,
) -> Result<bool, VerifyError> {
    let options = options.unwrap_or(VerifyOptions {
        max_age: Some(DEFAULT_MAX_AGE),
    });

    // Check timestamp expiration if max_age is set
    if let Some(max_age) = options.max_age {
        let timestamp = extract_timestamp(&params.nonce);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        if timestamp > now {
            return Err(VerifyError::FutureTimestamp);
        }

        let age = now - timestamp;
        let max_age_ms = max_age.as_millis() as u64;
        if age > max_age_ms {
            return Err(VerifyError::Expired {
                age_ms: age,
                max_age_ms,
            });
        }
    }

    // Only Ed25519 is supported
    if signed_message.public_key.key_type() != KeyType::Ed25519 {
        return Err(VerifyError::UnsupportedKeyType);
    }

    // Reconstruct the hashed payload
    let hash = serialize_message(params);

    // Decode the signature
    let signature_bytes = decode_signature(&signed_message.signature)?;

    // Verify the signature
    let pk_bytes = signed_message
        .public_key
        .as_ed25519_bytes()
        .ok_or_else(|| VerifyError::InvalidPublicKey("Not an Ed25519 key".to_string()))?;

    let verifying_key = VerifyingKey::from_bytes(pk_bytes)
        .map_err(|e| VerifyError::InvalidPublicKey(e.to_string()))?;

    let sig_array: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| VerifyError::InvalidSignatureEncoding)?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    match verifying_key.verify_strict(&hash, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Err(VerifyError::SignatureInvalid),
    }
}

/// Decode a signature from various formats.
///
/// Supports:
/// - Base64 (NEP-413 spec)
/// - Prefixed base58 (ed25519:... or secp256k1:...) for backward compatibility
/// - Unprefixed base58 for backward compatibility
fn decode_signature(signature: &str) -> Result<Vec<u8>, VerifyError> {
    use base64::Engine;

    // Try base64 first (NEP-413 spec)
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(signature) {
        return Ok(bytes);
    }

    // Try prefixed base58 (ed25519:... or secp256k1:...)
    if let Some(data) = signature.strip_prefix("ed25519:") {
        if let Ok(bytes) = bs58::decode(data).into_vec() {
            return Ok(bytes);
        }
    }
    if let Some(data) = signature.strip_prefix("secp256k1:") {
        if let Ok(bytes) = bs58::decode(data).into_vec() {
            return Ok(bytes);
        }
    }

    // Try unprefixed base58
    if signature
        .chars()
        .all(|c| matches!(c, '1'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='Z' | 'a'..='k' | 'm'..='z'))
    {
        if let Ok(bytes) = bs58::decode(signature).into_vec() {
            return Ok(bytes);
        }
    }

    Err(VerifyError::InvalidSignatureEncoding)
}

/// Sign a NEP-413 message.
///
/// This is a low-level function. Prefer using `Near::sign_message()` which
/// handles account ID and key management automatically.
pub fn sign_message(
    secret_key: &SecretKey,
    account_id: &AccountId,
    params: &SignMessageParams,
) -> Result<SignedMessage, Nep413Error> {
    // Only Ed25519 is supported
    if secret_key.key_type() != KeyType::Ed25519 {
        return Err(Nep413Error::UnsupportedKeyType(secret_key.key_type()));
    }

    // Serialize and hash the message
    let hash = serialize_message(params);

    // Sign the hash
    let signature = secret_key.sign(&hash);

    // Encode signature as base64 (NEP-413 spec)
    use base64::Engine;
    let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.as_bytes());

    Ok(SignedMessage {
        account_id: account_id.clone(),
        public_key: secret_key.public_key(),
        signature: signature_b64,
        nonce: params.nonce,
        state: params.state.clone(),
    })
}

impl SecretKey {
    /// Sign a NEP-413 message.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::{SecretKey, AccountId};
    /// use near_kit::nep413::{generate_nonce, SignMessageParams};
    ///
    /// let secret_key = SecretKey::generate_ed25519();
    /// let account_id: AccountId = "alice.near".parse().unwrap();
    /// let nonce = generate_nonce();
    ///
    /// let signed = secret_key.sign_nep413_message(
    ///     &account_id,
    ///     &SignMessageParams {
    ///         message: "Login to MyApp".to_string(),
    ///         recipient: "myapp.com".to_string(),
    ///         nonce,
    ///         callback_url: None,
    ///         state: None,
    ///     },
    /// ).unwrap();
    /// ```
    pub fn sign_nep413_message(
        &self,
        account_id: &AccountId,
        params: &SignMessageParams,
    ) -> Result<SignedMessage, Nep413Error> {
        sign_message(self, account_id, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nep413_tag() {
        // 2^31 + 413 = 2147484061
        assert_eq!(NEP413_TAG, 2147484061);
        assert_eq!(NEP413_TAG, (1u32 << 31) + 413);
    }

    #[test]
    fn test_generate_nonce() {
        let nonce = generate_nonce();
        assert_eq!(nonce.len(), 32);

        // First 8 bytes should be a valid timestamp
        let timestamp = extract_timestamp(&nonce);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Timestamp should be within 1 second of now
        assert!(timestamp <= now);
        assert!(now - timestamp < 1000);
    }

    #[test]
    fn test_sign_and_verify() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Verify structure
        assert_eq!(signed.account_id, account_id);
        assert_eq!(signed.public_key, secret_key.public_key());
        assert!(!signed.signature.is_empty());

        // Verify signature (disable timestamp check since we just created it)
        let result = verify_signature(&signed, &params, Some(VerifyOptions { max_age: None }));
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_wrong_message_fails() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Try to verify with different message
        let wrong_params = SignMessageParams {
            message: "Different message".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let result = verify_signature(
            &signed,
            &wrong_params,
            Some(VerifyOptions { max_age: None }),
        );
        assert!(matches!(result, Err(VerifyError::SignatureInvalid)));
    }

    #[test]
    fn test_wrong_nonce_fails() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Try to verify with different nonce
        let wrong_params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce: generate_nonce(), // Different nonce
            callback_url: None,
            state: None,
        };

        let result = verify_signature(
            &signed,
            &wrong_params,
            Some(VerifyOptions { max_age: None }),
        );
        assert!(matches!(result, Err(VerifyError::SignatureInvalid)));
    }

    #[test]
    fn test_deterministic_signatures() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = [42u8; 32];

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed1 = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();
        let signed2 = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Ed25519 signatures are deterministic
        assert_eq!(signed1.signature, signed2.signature);
    }

    #[test]
    fn test_empty_message() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: String::new(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();
        let result = verify_signature(&signed, &params, Some(VerifyOptions { max_age: None }));
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_unicode_message() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "こんにちは世界 🌍 Привет мир".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();
        let result = verify_signature(&signed, &params, Some(VerifyOptions { max_age: None }));
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_signature_is_base64() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Verify it's valid base64
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD.decode(&signed.signature);
        assert!(decoded.is_ok());
        assert_eq!(decoded.unwrap().len(), 64); // Ed25519 signature is 64 bytes
    }

    #[test]
    fn test_legacy_base58_signature_verification() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();
        let nonce = generate_nonce();

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Convert to legacy base58 format
        use base64::Engine;
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&signed.signature)
            .unwrap();
        let base58_sig = format!("ed25519:{}", bs58::encode(&sig_bytes).into_string());

        let legacy_signed = SignedMessage {
            account_id: signed.account_id,
            public_key: signed.public_key,
            signature: base58_sig,
            nonce: signed.nonce,
            state: None,
        };

        let result = verify_signature(
            &legacy_signed,
            &params,
            Some(VerifyOptions { max_age: None }),
        );
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_expired_signature() {
        let secret_key = SecretKey::generate_ed25519();
        let account_id: AccountId = "test.near".parse().unwrap();

        // Create a nonce with an old timestamp (1 hour ago)
        let mut nonce = [0u8; 32];
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 3_600_000; // 1 hour ago
        nonce[..8].copy_from_slice(&old_timestamp.to_be_bytes());
        rand::thread_rng().fill_bytes(&mut nonce[8..]);

        let params = SignMessageParams {
            message: "Login to MyApp".to_string(),
            recipient: "myapp.near".to_string(),
            nonce,
            callback_url: None,
            state: None,
        };

        let signed = secret_key
            .sign_nep413_message(&account_id, &params)
            .unwrap();

        // Verify with default 5 minute max age
        let result = verify_signature(&signed, &params, None);
        assert!(matches!(result, Err(VerifyError::Expired { .. })));
    }
}
