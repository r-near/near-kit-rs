//! Keyring-based signer using system credential storage.
//!
//! This module provides [`KeyringSigner`], which loads keys from the operating system's
//! native credential storage (macOS Keychain, Windows Credential Manager, or Linux
//! Secret Service).
//!
//! # Compatibility
//!
//! `KeyringSigner` is fully compatible with keys stored by `near-cli-rs`. If you've
//! imported keys using `near-cli-rs`, you can use them directly with `near-kit-rs`.
//!
//! # Platform Support
//!
//! - **macOS**: Uses Keychain (works out of the box)
//! - **Windows**: Uses Credential Manager (works out of the box)
//! - **Linux**: Uses Secret Service D-Bus API (requires `gnome-keyring`, `kwallet`, or similar)
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::{KeyringSigner, Near};
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! // Load a key stored by near-cli-rs
//! let signer = KeyringSigner::new(
//!     "testnet",
//!     "alice.testnet",
//!     "ed25519:6fWy..."
//! )?;
//!
//! let near = Near::testnet().signer(signer).build();
//! near.transfer("bob.testnet", "1 NEAR").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Importing Keys
//!
//! Use `near-cli-rs` to import keys to the system keyring:
//!
//! ```bash
//! near account import-account using-seed-phrase "your seed phrase here" \
//!   --seed-phrase-hd-path "m/44'/397'/0'" \
//!   network-config testnet
//! ```

use crate::client::signer::{InMemorySigner, Nep413SignFuture, SignFuture, Signer};
use crate::error::{Error, KeyStoreError, ParseKeyError};
use crate::types::{AccountId, PublicKey, SecretKey};

/// Signer that loads keys from the system keyring.
///
/// Compatible with keys stored by `near-cli-rs`.
///
/// # Construction
///
/// Unlike [`FileSigner`](crate::FileSigner), `KeyringSigner` requires the public key
/// because keyring entries are keyed by `{account_id}:{public_key}`.
///
/// ```rust,no_run
/// use near_kit::KeyringSigner;
///
/// let signer = KeyringSigner::new(
///     "testnet",
///     "alice.testnet",
///     "ed25519:6fWy..."
/// )?;
/// # Ok::<(), near_kit::Error>(())
/// ```
#[derive(Clone)]
pub struct KeyringSigner {
    inner: InMemorySigner,
}

impl KeyringSigner {
    /// Load a key from the system keyring.
    ///
    /// # Arguments
    ///
    /// * `network` - Network name (e.g., "testnet", "mainnet")
    /// * `account_id` - The NEAR account ID
    /// * `public_key` - The public key to look up (e.g., "ed25519:...")
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The keyring is not available (e.g., no Secret Service on Linux)
    /// - The key is not found in the keyring
    /// - The stored credential has an invalid format
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::KeyringSigner;
    ///
    /// let signer = KeyringSigner::new(
    ///     "testnet",
    ///     "alice.testnet",
    ///     "ed25519:6fWy..."
    /// )?;
    /// # Ok::<(), near_kit::Error>(())
    /// ```
    pub fn new(
        network: impl AsRef<str>,
        account_id: impl AsRef<str>,
        public_key: impl AsRef<str>,
    ) -> Result<Self, Error> {
        let network = network.as_ref();
        let account_id_str = account_id.as_ref();
        let public_key_str = public_key.as_ref();

        // Parse account ID and public key for validation
        let account_id: AccountId = account_id_str.parse()?;
        let public_key: PublicKey = public_key_str.parse()?;

        // Construct keyring entry using near-cli-rs format
        // Service: "near-{network}-{account_id}"
        // Username: "{account_id}:{public_key}"
        let service_name = format!("near-{}-{}", network, account_id_str);
        let username = format!("{}:{}", account_id_str, public_key_str);

        let entry = keyring::Entry::new(&service_name, &username).map_err(|e| {
            Error::KeyStore(KeyStoreError::Platform(format!(
                "Failed to access keyring: {}. On Linux, ensure a Secret Service daemon \
                 (gnome-keyring, kwallet) is running.",
                e
            )))
        })?;

        let password = entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => {
                Error::KeyStore(KeyStoreError::KeyNotFound(account_id.clone()))
            }
            _ => Error::KeyStore(KeyStoreError::Platform(format!(
                "Failed to read from keyring: {}",
                e
            ))),
        })?;

        // Parse the stored JSON credential
        let secret_key = parse_keyring_credential(&password, &account_id, &public_key)?;

        // Create the inner signer
        let inner = InMemorySigner::from_secret_key(account_id, secret_key);

        // Verify the public key matches
        if inner.public_key() != &public_key {
            return Err(Error::KeyStore(KeyStoreError::InvalidFormat(format!(
                "Public key mismatch: stored key has {}, but requested {}",
                inner.public_key(),
                public_key
            ))));
        }

        Ok(Self { inner })
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }
}

impl std::fmt::Debug for KeyringSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringSigner")
            .field("account_id", self.inner.account_id())
            .field("public_key", self.inner.public_key())
            .finish()
    }
}

impl Signer for KeyringSigner {
    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        self.inner.sign(message)
    }

    fn claim_key(&self) -> (crate::PublicKey, Box<dyn super::signer::ClaimedKey + Send>) {
        self.inner.claim_key()
    }

    fn sign_nep413<'a>(
        &'a self,
        params: &'a crate::types::nep413::SignMessageParams,
    ) -> Nep413SignFuture<'a> {
        self.inner.sign_nep413(params)
    }
}

// ============================================================================
// Credential Parsing
// ============================================================================

/// Parse a keyring credential JSON into a SecretKey.
///
/// Supports two formats used by near-cli-rs:
///
/// 1. Full format (from seed phrase import):
/// ```json
/// {
///   "seed_phrase_hd_path": "m/44'/397'/0'",
///   "master_seed_phrase": "word1 word2 ...",
///   "implicit_account_id": "...",
///   "public_key": "ed25519:...",
///   "private_key": "ed25519:..."
/// }
/// ```
///
/// 2. Simple format (from private key import):
/// ```json
/// {
///   "public_key": "ed25519:...",
///   "private_key": "ed25519:..."
/// }
/// ```
fn parse_keyring_credential(
    json_str: &str,
    account_id: &AccountId,
    _public_key: &PublicKey,
) -> Result<SecretKey, Error> {
    // Try to parse as JSON
    let value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        Error::KeyStore(KeyStoreError::InvalidFormat(format!(
            "Invalid JSON in keyring credential for {}: {}",
            account_id, e
        )))
    })?;

    // Extract private_key field (works for both formats)
    let private_key_str = value
        .get("private_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            Error::KeyStore(KeyStoreError::InvalidFormat(format!(
                "Missing 'private_key' field in keyring credential for {}",
                account_id
            )))
        })?;

    // Parse the secret key
    let secret_key: SecretKey = private_key_str
        .parse()
        .map_err(|e: ParseKeyError| Error::KeyStore(KeyStoreError::InvalidKey(e)))?;

    Ok(secret_key)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_format() {
        let json = r#"{
            "seed_phrase_hd_path": "m/44'/397'/0'",
            "master_seed_phrase": "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "implicit_account_id": "c4f5941e81e071c2fd1dae2e71fd3d859d462484391d9a90bf219211dcbb320f",
            "public_key": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847",
            "private_key": "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"
        }"#;

        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
            .parse()
            .unwrap();

        let secret_key = parse_keyring_credential(json, &account_id, &public_key).unwrap();

        // Verify the key was parsed correctly
        assert!(secret_key.to_string().starts_with("ed25519:"));
    }

    #[test]
    fn test_parse_simple_format() {
        let json = r#"{
            "public_key": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847",
            "private_key": "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"
        }"#;

        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
            .parse()
            .unwrap();

        let secret_key = parse_keyring_credential(json, &account_id, &public_key).unwrap();

        // Verify the key was parsed correctly
        assert!(secret_key.to_string().starts_with("ed25519:"));
    }

    #[test]
    fn test_parse_missing_private_key() {
        let json = r#"{
            "public_key": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
        }"#;

        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
            .parse()
            .unwrap();

        let result = parse_keyring_credential(json, &account_id, &public_key);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("Missing 'private_key' field"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "not valid json";

        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
            .parse()
            .unwrap();

        let result = parse_keyring_credential(json, &account_id, &public_key);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_parse_invalid_key_format() {
        let json = r#"{
            "public_key": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847",
            "private_key": "not-a-valid-key"
        }"#;

        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
            .parse()
            .unwrap();

        let result = parse_keyring_credential(json, &account_id, &public_key);
        assert!(result.is_err());
    }
}
