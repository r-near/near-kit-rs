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

use crate::error::{Error, KeyStoreError};
use crate::types::{AccountId, InMemorySigner, PublicKey, SecretKey};

/// Load the key `public_key` of `account_id` on `network` from the system
/// keyring, where `near-cli-rs` stores it.
///
/// The keyring is read once; the result is a plain [`InMemorySigner`].
/// Keyring entries are keyed by `{account_id}:{public_key}` (for ML-DSA-65,
/// near-cli-rs 0.31+ uses the key's `ml-dsa-65-hash:` handle instead; both
/// are tried, the handle first), so unlike
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
///     .await?.result()?;
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
    // username: "{account_id}:{key id}". The key id is the full public key,
    // except that near-cli-rs 0.31+ names ML-DSA-65 entries after the
    // `ml-dsa-65-hash:` handle. Try the handle first: Windows Credential
    // Manager rejects the ~2,700-character full-key username with an error
    // rather than "not found", which would end the search.
    let service_name = format!("near-{network}-{account_id}");
    let mut usernames = Vec::with_capacity(2);
    if let Some(handle) = public_key.to_ml_dsa65_hash() {
        usernames.push(format!("{account_id}:{handle}"));
    }
    usernames.push(format!("{account_id}:{public_key}"));

    let mut password = None;
    for username in &usernames {
        let entry = keyring::Entry::new(&service_name, username).map_err(|e| {
            Error::KeyStore(KeyStoreError::Platform(format!(
                "Failed to access keyring: {}. On Linux, ensure a Secret Service daemon \
                 (gnome-keyring, kwallet) is running.",
                e
            )))
        })?;
        match entry.get_password() {
            Ok(found) => {
                password = Some(found);
                break;
            }
            Err(keyring::Error::NoEntry) => continue,
            Err(e) => {
                return Err(Error::KeyStore(KeyStoreError::Platform(format!(
                    "Failed to read from keyring: {}",
                    e
                ))));
            }
        }
    }
    let password =
        password.ok_or_else(|| Error::KeyStore(KeyStoreError::KeyNotFound(account_id.clone())))?;

    let secret_key = parse_keyring_credential(&password, &account_id)?;
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

/// Parse the `private_key` of a near-cli-rs keyring credential (the other
/// fields vary by how the key was imported and are ignored).
fn parse_keyring_credential(
    json_str: &str,
    account_id: &AccountId,
) -> Result<SecretKey, KeyStoreError> {
    let value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        KeyStoreError::InvalidFormat(format!(
            "Invalid JSON in keyring credential for {account_id}: {e}"
        ))
    })?;
    let private_key = value["private_key"].as_str().ok_or_else(|| {
        KeyStoreError::InvalidFormat(format!(
            "Missing 'private_key' field in keyring credential for {account_id}"
        ))
    })?;
    Ok(private_key.parse()?)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signer::Signer as _;

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

        let secret_key = parse_keyring_credential(json, &account_id).unwrap();

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

        let secret_key = parse_keyring_credential(json, &account_id).unwrap();

        // Verify the key was parsed correctly
        assert!(secret_key.to_string().starts_with("ed25519:"));
    }

    #[test]
    fn test_parse_missing_private_key() {
        let json = r#"{
            "public_key": "ed25519:DcA2MzgpJbrUATQLLceocVckhhAqrkingax4oJ9kZ847"
        }"#;

        let account_id: AccountId = "alice.testnet".parse().unwrap();

        let result = parse_keyring_credential(json, &account_id);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("Missing 'private_key' field"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "not valid json";

        let account_id: AccountId = "alice.testnet".parse().unwrap();

        let result = parse_keyring_credential(json, &account_id);
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

        let result = parse_keyring_credential(json, &account_id);
        assert!(result.is_err());
    }

    /// An in-memory keyring shared by every entry, installed as the default
    /// store so these tests never touch the OS keychain. (`keyring::mock`
    /// keeps each password inside its own `Entry`, so a value set through one
    /// entry can't be read through another.)
    mod memory_store {
        use std::any::Any;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex, OnceLock};

        use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};

        type Store = Arc<Mutex<HashMap<(String, String), Slot>>>;

        /// What an entry holds.
        #[derive(Debug, Clone)]
        enum Slot {
            Secret(Vec<u8>),
            /// Reading fails the way Windows Credential Manager rejects a
            /// username over its 513-character limit.
            UsernameTooLong,
        }

        #[derive(Debug)]
        struct MemoryCredential {
            store: Store,
            key: (String, String),
        }

        impl CredentialApi for MemoryCredential {
            fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
                self.store
                    .lock()
                    .unwrap()
                    .insert(self.key.clone(), Slot::Secret(secret.to_vec()));
                Ok(())
            }

            fn get_secret(&self) -> keyring::Result<Vec<u8>> {
                match self.store.lock().unwrap().get(&self.key) {
                    Some(Slot::Secret(secret)) => Ok(secret.clone()),
                    Some(Slot::UsernameTooLong) => {
                        Err(keyring::Error::TooLong("user".to_string(), 513))
                    }
                    None => Err(keyring::Error::NoEntry),
                }
            }

            fn delete_credential(&self) -> keyring::Result<()> {
                self.store
                    .lock()
                    .unwrap()
                    .remove(&self.key)
                    .map(|_| ())
                    .ok_or(keyring::Error::NoEntry)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        #[derive(Debug)]
        struct MemoryBuilder(Store);

        impl CredentialBuilderApi for MemoryBuilder {
            fn build(
                &self,
                _target: Option<&str>,
                service: &str,
                user: &str,
            ) -> keyring::Result<Box<Credential>> {
                Ok(Box::new(MemoryCredential {
                    store: self.0.clone(),
                    key: (service.to_string(), user.to_string()),
                }))
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        static STORE: OnceLock<Store> = OnceLock::new();

        /// The shared store, installed as the default credential store on
        /// first use.
        fn store() -> &'static Store {
            STORE.get_or_init(|| {
                let store = Store::default();
                keyring::set_default_credential_builder(Box::new(MemoryBuilder(store.clone())));
                store
            })
        }

        /// Store `password` under `service`/`user`.
        pub(super) fn put(service: &str, user: &str, password: &str) {
            store();
            keyring::Entry::new(service, user)
                .unwrap()
                .set_password(password)
                .unwrap();
        }

        /// Make reading `service`/`user` fail as Windows does for a username
        /// that is too long.
        pub(super) fn fail_reads(service: &str, user: &str) {
            store().lock().unwrap().insert(
                (service.to_string(), user.to_string()),
                Slot::UsernameTooLong,
            );
        }
    }

    /// The JSON near-cli-rs stores for a key.
    fn credential_json(secret_key: &str, public_key: &PublicKey) -> String {
        serde_json::json!({
            "public_key": public_key.to_string(),
            "private_key": secret_key,
        })
        .to_string()
    }

    /// `ml-dsa-65:<base58 of the 4032-byte expanded key>`, as near-cli-rs writes it.
    fn expanded_ml_dsa65_string(secret_key: &SecretKey) -> String {
        format!(
            "ml-dsa-65:{}",
            bs58::encode(secret_key.to_ml_dsa65_expanded_bytes().unwrap().as_slice()).into_string()
        )
    }

    #[test]
    fn ml_dsa65_key_stored_under_its_hash_handle_loads() {
        // near-cli-rs 0.31 names the entry after the `ml-dsa-65-hash:` handle.
        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let secret_key = SecretKey::ml_dsa65_from_seed([11; 32]);
        let public_key = secret_key.public_key();
        let handle = public_key.to_ml_dsa65_hash().unwrap();
        memory_store::put(
            "near-kitnet-handle-alice.testnet",
            &format!("{account_id}:{handle}"),
            &credential_json(&expanded_ml_dsa65_string(&secret_key), &public_key),
        );

        let signer = from_keyring("kitnet-handle", account_id, &public_key).unwrap();
        assert_eq!(signer.public_key(), public_key);
    }

    #[test]
    fn ml_dsa65_hash_handle_is_tried_before_the_full_key() {
        // Windows Credential Manager rejects the full-key username (about
        // 2,700 characters) before near-kit could fall back to the handle.
        let account_id: AccountId = "dave.testnet".parse().unwrap();
        let secret_key = SecretKey::ml_dsa65_from_seed([17; 32]);
        let public_key = secret_key.public_key();
        let handle = public_key.to_ml_dsa65_hash().unwrap();
        let service = "near-kitnet-order-dave.testnet";
        memory_store::put(
            service,
            &format!("{account_id}:{handle}"),
            &credential_json(&expanded_ml_dsa65_string(&secret_key), &public_key),
        );
        memory_store::fail_reads(service, &format!("{account_id}:{public_key}"));

        let signer = from_keyring("kitnet-order", account_id, &public_key).unwrap();
        assert_eq!(signer.public_key(), public_key);
    }

    #[test]
    fn ml_dsa65_expanded_key_stored_under_the_full_key_loads() {
        // Earlier near-cli-rs releases name the entry after the full key.
        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let secret_key = SecretKey::ml_dsa65_from_seed([12; 32]);
        let public_key = secret_key.public_key();
        memory_store::put(
            "near-kitnet-full-alice.testnet",
            &format!("{account_id}:{public_key}"),
            &credential_json(&expanded_ml_dsa65_string(&secret_key), &public_key),
        );

        let signer = from_keyring("kitnet-full", account_id, &public_key).unwrap();
        assert_eq!(signer.public_key(), public_key);
    }

    #[test]
    fn ed25519_key_loads_and_missing_key_is_not_found() {
        let account_id: AccountId = "bob.testnet".parse().unwrap();
        let secret_key = SecretKey::ed25519_from_bytes([13; 32]);
        let public_key = secret_key.public_key();
        memory_store::put(
            "near-kitnet-ed-bob.testnet",
            &format!("{account_id}:{public_key}"),
            &credential_json(&secret_key.to_string(), &public_key),
        );
        let signer = from_keyring("kitnet-ed", account_id.clone(), &public_key).unwrap();
        assert_eq!(signer.public_key(), public_key);

        let other = SecretKey::ml_dsa65_from_seed([14; 32]).public_key();
        assert!(matches!(
            from_keyring("kitnet-ed", account_id, &other),
            Err(Error::KeyStore(KeyStoreError::KeyNotFound(_)))
        ));
    }

    #[test]
    fn wrong_key_under_the_hash_handle_is_rejected() {
        let account_id: AccountId = "carol.testnet".parse().unwrap();
        let public_key = SecretKey::ml_dsa65_from_seed([15; 32]).public_key();
        let other = SecretKey::ml_dsa65_from_seed([16; 32]);
        memory_store::put(
            "near-kitnet-mismatch-carol.testnet",
            &format!("{account_id}:{}", public_key.to_ml_dsa65_hash().unwrap()),
            &credential_json(&other.to_string(), &other.public_key()),
        );

        let error = from_keyring("kitnet-mismatch", account_id, &public_key).unwrap_err();
        assert!(error.to_string().contains("Public key mismatch"), "{error}");
    }
}
