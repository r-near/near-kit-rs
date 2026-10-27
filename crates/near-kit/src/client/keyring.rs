//! Load an [`InMemorySigner`] from the operating system's credential store
//! (macOS Keychain, Windows Credential Manager, or Linux Secret Service).
//!
//! # Compatibility
//!
//! [`from_keyring`] reads keys stored by `near-cli-rs`. If you've imported
//! keys with `near-cli-rs`, you can use them directly with near-kit.
//!
//! # Platform Support
//!
//! - **macOS**: Uses Keychain (works out of the box)
//! - **Windows**: Uses Credential Manager (works out of the box)
//! - **Linux**: Uses Secret Service D-Bus API (requires `gnome-keyring`, `kwallet`, or similar)
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

use crate::error::{Error, KeyStoreError, ParseKeyError};
use crate::types::{AccountId, InMemorySigner, PublicKey, SecretKey};

/// Load the key `public_key` of `account_id` on `network` from the system
/// keyring, where `near-cli-rs` stores it.
///
/// The keyring is read once; the result is a plain [`InMemorySigner`].
/// Keyring entries are keyed by `{account_id}:{public_key}`, so unlike
/// [`from_credentials`](crate::signer::from_credentials) this needs the
/// public key. Requires the opt-in `keyring` feature.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::protocol::Action;
/// use near_kit::signer::{self, PublicKey};
/// use near_kit::{Near, NearToken, account};
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let public_key: PublicKey = "ed25519:6fWy...".parse()?;
/// let signer = signer::from_keyring("testnet", account!("alice.testnet"), &public_key)?;
///
/// let alice = Near::testnet().build().with_signer(signer);
/// alice
///     .transaction(account!("bob.testnet"))
///     .add_action(Action::transfer(NearToken::from_near(1)))
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`Error::KeyStore`] if the keyring is not available (e.g. no Secret
/// Service on Linux), the key is not found, the stored credential has an
/// invalid format, or it holds a different key.
pub fn from_keyring(
    network: &str,
    account_id: impl Into<AccountId>,
    public_key: &PublicKey,
) -> Result<InMemorySigner, Error> {
    let account_id: AccountId = account_id.into();

    // near-cli-rs entry format. Service: "near-{network}-{account_id}",
    // username: "{account_id}:{public_key}".
    let service_name = format!("near-{network}-{account_id}");
    let username = format!("{account_id}:{public_key}");

    let entry = keyring::Entry::new(&service_name, &username).map_err(|e| {
        Error::KeyStore(KeyStoreError::Platform(format!(
            "Failed to access keyring: {}. On Linux, ensure a Secret Service daemon \
             (gnome-keyring, kwallet) is running.",
            e
        )))
    })?;

    let password = entry.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => Error::KeyStore(KeyStoreError::KeyNotFound(account_id.clone())),
        _ => Error::KeyStore(KeyStoreError::Platform(format!(
            "Failed to read from keyring: {}",
            e
        ))),
    })?;

    let secret_key = parse_keyring_credential(&password, &account_id, public_key)?;
    if &secret_key.public_key() != public_key {
        return Err(Error::KeyStore(KeyStoreError::InvalidFormat(format!(
            "Public key mismatch: stored key has {}, but requested {}",
            secret_key.public_key(),
            public_key
        ))));
    }

    Ok(InMemorySigner::new(account_id, secret_key))
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
