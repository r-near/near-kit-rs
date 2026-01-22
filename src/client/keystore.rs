//! Key storage implementations.
//!
//! A `KeyStore` manages private keys for multiple accounts. Different implementations
//! support different storage backends:
//!
//! - [`InMemoryKeyStore`] - Ephemeral storage in RAM (testing, scripts)
//! - [`FileKeyStore`] - NEAR CLI compatible file storage (`~/.near-credentials`)
//! - `RotatingKeyStore` - Multiple keys per account for high concurrency (coming soon)

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::KeyStoreError;
use crate::types::{AccountId, PublicKey, SecretKey};

/// A key store manages secret keys for multiple accounts.
///
/// Implementations can store keys in memory, on disk, in a hardware
/// security module, or any other secure storage backend.
pub trait KeyStore: Send + Sync {
    /// Add a key for an account.
    fn add(&self, account_id: &AccountId, secret_key: SecretKey);

    /// Get the key for an account, if it exists.
    fn get(&self, account_id: &AccountId) -> Option<SecretKey>;

    /// Remove the key for an account.
    fn remove(&self, account_id: &AccountId);

    /// List all account IDs with stored keys.
    fn list(&self) -> Vec<AccountId>;

    /// Check if a key exists for an account.
    fn contains(&self, account_id: &AccountId) -> bool {
        self.get(account_id).is_some()
    }
}

/// In-memory key store.
///
/// Keys are stored in memory and lost when the process exits.
/// Useful for testing, development, and temporary key storage.
///
/// # Example
///
/// ```rust
/// use near_kit::{InMemoryKeyStore, KeyStore, SecretKey};
///
/// let keystore = InMemoryKeyStore::new();
///
/// // Add a key
/// let secret = SecretKey::generate_ed25519();
/// let account_id = "alice.testnet".parse().unwrap();
/// keystore.add(&account_id, secret);
///
/// // Retrieve it later
/// let key = keystore.get(&account_id);
/// assert!(key.is_some());
/// ```
#[derive(Debug, Default)]
pub struct InMemoryKeyStore {
    keys: RwLock<HashMap<AccountId, SecretKey>>,
}

impl InMemoryKeyStore {
    /// Create an empty in-memory keystore.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a keystore pre-populated with keys.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::InMemoryKeyStore;
    ///
    /// let keystore = InMemoryKeyStore::from_keys(&[
    ///     ("alice.testnet", "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"),
    /// ]);
    /// ```
    pub fn from_keys(keys: &[(&str, &str)]) -> Self {
        let store = Self::new();
        for (account_id, secret_key) in keys {
            if let (Ok(account_id), Ok(secret_key)) = (account_id.parse(), secret_key.parse()) {
                store.add(&account_id, secret_key);
            }
        }
        store
    }

    /// Clear all keys from the store.
    pub fn clear(&self) {
        self.keys.write().unwrap().clear();
    }

    /// Get the number of stored keys.
    pub fn len(&self) -> usize {
        self.keys.read().unwrap().len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.keys.read().unwrap().is_empty()
    }
}

impl KeyStore for InMemoryKeyStore {
    fn add(&self, account_id: &AccountId, secret_key: SecretKey) {
        self.keys
            .write()
            .unwrap()
            .insert(account_id.clone(), secret_key);
    }

    fn get(&self, account_id: &AccountId) -> Option<SecretKey> {
        self.keys.read().unwrap().get(account_id).cloned()
    }

    fn remove(&self, account_id: &AccountId) {
        self.keys.write().unwrap().remove(account_id);
    }

    fn list(&self) -> Vec<AccountId> {
        self.keys.read().unwrap().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_keystore() {
        let store = InMemoryKeyStore::new();
        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        // Add key
        store.add(&account_id, secret);
        assert!(store.contains(&account_id));
        assert_eq!(store.len(), 1);

        // Get key
        let retrieved = store.get(&account_id).unwrap();
        assert_eq!(retrieved.public_key(), public);

        // List keys
        let accounts = store.list();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0], account_id);

        // Remove key
        store.remove(&account_id);
        assert!(!store.contains(&account_id));
        assert!(store.is_empty());
    }

    #[test]
    fn test_from_keys() {
        let store = InMemoryKeyStore::from_keys(&[
            ("alice.testnet", "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"),
        ]);
        assert_eq!(store.len(), 1);
        assert!(store.contains(&"alice.testnet".parse().unwrap()));
    }
}

// ============================================================================
// FileKeyStore - near-cli-rs compatible filesystem keystore
// ============================================================================

/// Credential file format compatible with `near-cli` and `near-cli-rs`.
///
/// Files are stored as JSON with these fields:
/// ```json
/// {
///   "account_id": "alice.testnet",
///   "public_key": "ed25519:...",
///   "private_key": "ed25519:...",
///   "master_seed_phrase": "word1 word2 ... (optional)",
///   "seed_phrase_hd_path": "m/44'/397'/0' (optional)",
///   "implicit_account_id": "hex... (optional)"
/// }
/// ```
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NearCliCredential {
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    public_key: String,
    private_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    master_seed_phrase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_phrase_hd_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    implicit_account_id: Option<String>,
}

/// Legacy credential format that uses `secret_key` instead of `private_key`.
/// Supported for reading only.
#[allow(dead_code)] // Only secret_key is used; other fields kept for format compatibility
#[derive(Debug, serde::Deserialize)]
struct LegacyCredential {
    #[serde(default)]
    account_id: Option<String>,
    public_key: String,
    secret_key: String,
    #[serde(default)]
    master_seed_phrase: Option<String>,
    #[serde(default)]
    seed_phrase_hd_path: Option<String>,
    #[serde(default)]
    implicit_account_id: Option<String>,
}

/// Filesystem-based key store compatible with `near-cli` and `near-cli-rs`.
///
/// Keys are stored in `~/.near-credentials/{network}/{account}.json` format.
///
/// # Directory Structure
///
/// ```text
/// ~/.near-credentials/
///   ├── mainnet/
///   │   └── account.near.json
///   ├── testnet/
///   │   ├── account.testnet.json
///   │   └── account.testnet/              # Multi-key format (read-only)
///   │       └── ed25519_PublicKey.json
///   └── implicit/
///       └── accountId.json
/// ```
///
/// # Compatibility
///
/// - **Writing**: Uses the modern format with `private_key` field
/// - **Reading**: Supports both `private_key` (modern) and `secret_key` (legacy) formats
/// - **Multi-key**: Reads from `{account}/{key_type}_{public_key}.json` subdirectories
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::{FileKeyStore, KeyStore, SecretKey};
///
/// // Use default path (~/.near-credentials) with testnet network
/// let keystore = FileKeyStore::new("testnet").unwrap();
///
/// // Or specify a custom path
/// let keystore = FileKeyStore::with_path("/custom/path", "mainnet").unwrap();
///
/// // Add a key (creates the file)
/// let secret = SecretKey::generate_ed25519();
/// let account_id = "alice.testnet".parse().unwrap();
/// keystore.add(&account_id, secret);
///
/// // Retrieve it later
/// let key = keystore.get(&account_id);
/// assert!(key.is_some());
/// ```
#[derive(Debug)]
pub struct FileKeyStore {
    /// Base path to credentials directory (e.g., ~/.near-credentials)
    base_path: PathBuf,
    /// Network subdirectory (e.g., "testnet", "mainnet")
    network: String,
}

impl FileKeyStore {
    /// Create a new FileKeyStore using the default path (`~/.near-credentials`).
    ///
    /// # Arguments
    ///
    /// * `network` - Network name (e.g., "testnet", "mainnet")
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::FileKeyStore;
    ///
    /// let keystore = FileKeyStore::new("testnet").unwrap();
    /// ```
    pub fn new(network: impl Into<String>) -> Result<Self, KeyStoreError> {
        let home = dirs::home_dir().ok_or_else(|| {
            KeyStoreError::PathError("Could not determine home directory".to_string())
        })?;
        let base_path = home.join(".near-credentials");
        Ok(Self {
            base_path,
            network: network.into(),
        })
    }

    /// Create a FileKeyStore with a custom base path.
    ///
    /// # Arguments
    ///
    /// * `path` - Base path for credentials (network will be a subdirectory)
    /// * `network` - Network name (e.g., "testnet", "mainnet")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::FileKeyStore;
    ///
    /// let keystore = FileKeyStore::with_path("/custom/path", "mainnet").unwrap();
    /// ```
    pub fn with_path(
        path: impl AsRef<Path>,
        network: impl Into<String>,
    ) -> Result<Self, KeyStoreError> {
        Ok(Self {
            base_path: path.as_ref().to_path_buf(),
            network: network.into(),
        })
    }

    /// Get the network-specific directory path.
    fn network_path(&self) -> PathBuf {
        self.base_path.join(&self.network)
    }

    /// Get the path to a single-key credential file for an account.
    fn key_file_path(&self, account_id: &AccountId) -> PathBuf {
        self.network_path().join(format!("{}.json", account_id))
    }

    /// Get the path to a multi-key directory for an account.
    fn multi_key_dir(&self, account_id: &AccountId) -> PathBuf {
        self.network_path().join(account_id.as_str())
    }

    /// Get the base path.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Get the network name.
    pub fn network(&self) -> &str {
        &self.network
    }

    /// Try to read a credential from the single-key format file.
    fn read_single_key(&self, account_id: &AccountId) -> Result<SecretKey, KeyStoreError> {
        let path = self.key_file_path(account_id);
        self.read_credential_file(&path)
    }

    /// Try to read a credential from the multi-key directory format.
    /// Returns the first valid key found.
    fn read_multi_key(&self, account_id: &AccountId) -> Result<SecretKey, KeyStoreError> {
        let dir_path = self.multi_key_dir(account_id);

        if !dir_path.is_dir() {
            return Err(KeyStoreError::KeyNotFound(account_id.clone()));
        }

        // Scan for key files (ed25519_*.json, secp256k1_*.json)
        let entries = fs::read_dir(&dir_path)?;

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            if file_name_str.ends_with(".json")
                && (file_name_str.starts_with("ed25519_")
                    || file_name_str.starts_with("secp256k1_"))
            {
                if let Ok(key) = self.read_credential_file(&entry.path()) {
                    return Ok(key);
                }
            }
        }

        Err(KeyStoreError::KeyNotFound(account_id.clone()))
    }

    /// Read and parse a credential file.
    fn read_credential_file(&self, path: &Path) -> Result<SecretKey, KeyStoreError> {
        let mut file = fs::File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        // Try modern format first (private_key)
        if let Ok(cred) = serde_json::from_str::<NearCliCredential>(&contents) {
            return cred.private_key.parse().map_err(KeyStoreError::InvalidKey);
        }

        // Try legacy format (secret_key)
        if let Ok(legacy) = serde_json::from_str::<LegacyCredential>(&contents) {
            return legacy.secret_key.parse().map_err(KeyStoreError::InvalidKey);
        }

        Err(KeyStoreError::InvalidFormat(
            "Credential file does not match expected format".to_string(),
        ))
    }

    /// Write a credential to a file.
    fn write_credential(
        &self,
        account_id: &AccountId,
        secret_key: &SecretKey,
    ) -> Result<(), KeyStoreError> {
        let network_path = self.network_path();
        fs::create_dir_all(&network_path)?;

        let credential = NearCliCredential {
            account_id: Some(account_id.to_string()),
            public_key: secret_key.public_key().to_string(),
            private_key: secret_key.to_string(),
            master_seed_phrase: None,
            seed_phrase_hd_path: None,
            implicit_account_id: None,
        };

        let json = serde_json::to_string_pretty(&credential)?;
        let path = self.key_file_path(account_id);

        let mut file = fs::File::create(&path)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }

    /// Add a key with additional metadata (seed phrase, derivation path).
    ///
    /// This method allows storing optional metadata along with the key,
    /// which is useful for backup and recovery purposes.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The account ID
    /// * `secret_key` - The secret key
    /// * `seed_phrase` - Optional BIP39 seed phrase
    /// * `derivation_path` - Optional BIP32 derivation path (e.g., "m/44'/397'/0'")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::{FileKeyStore, SecretKey};
    ///
    /// let keystore = FileKeyStore::new("testnet").unwrap();
    /// let secret = SecretKey::generate_ed25519();
    /// let account_id = "alice.testnet".parse().unwrap();
    ///
    /// keystore.add_with_metadata(
    ///     &account_id,
    ///     &secret,
    ///     Some("word1 word2 ... word12"),
    ///     Some("m/44'/397'/0'"),
    /// ).unwrap();
    /// ```
    pub fn add_with_metadata(
        &self,
        account_id: &AccountId,
        secret_key: &SecretKey,
        seed_phrase: Option<&str>,
        derivation_path: Option<&str>,
    ) -> Result<(), KeyStoreError> {
        let network_path = self.network_path();
        fs::create_dir_all(&network_path)?;

        // For implicit accounts, compute the implicit account ID
        let implicit_account_id = if account_id.is_implicit() {
            Some(account_id.to_string())
        } else {
            None
        };

        let credential = NearCliCredential {
            account_id: Some(account_id.to_string()),
            public_key: secret_key.public_key().to_string(),
            private_key: secret_key.to_string(),
            master_seed_phrase: seed_phrase.map(String::from),
            seed_phrase_hd_path: derivation_path.map(String::from),
            implicit_account_id,
        };

        let json = serde_json::to_string_pretty(&credential)?;
        let path = self.key_file_path(account_id);

        let mut file = fs::File::create(&path)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }

    /// Get all keys for an account (from multi-key directory).
    ///
    /// Returns all keys stored in the `{account}/` subdirectory.
    /// This is useful for accounts with multiple access keys.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::FileKeyStore;
    ///
    /// let keystore = FileKeyStore::new("testnet").unwrap();
    /// let account_id = "alice.testnet".parse().unwrap();
    ///
    /// for (public_key, secret_key) in keystore.get_all_keys(&account_id).unwrap() {
    ///     println!("Key: {}", public_key);
    /// }
    /// ```
    pub fn get_all_keys(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<(PublicKey, SecretKey)>, KeyStoreError> {
        let mut keys = Vec::new();

        // Check single-key file first
        if let Ok(key) = self.read_single_key(account_id) {
            keys.push((key.public_key(), key));
        }

        // Check multi-key directory
        let dir_path = self.multi_key_dir(account_id);
        if dir_path.is_dir() {
            if let Ok(entries) = fs::read_dir(&dir_path) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();

                    if file_name_str.ends_with(".json")
                        && (file_name_str.starts_with("ed25519_")
                            || file_name_str.starts_with("secp256k1_"))
                    {
                        if let Ok(key) = self.read_credential_file(&entry.path()) {
                            keys.push((key.public_key(), key));
                        }
                    }
                }
            }
        }

        if keys.is_empty() {
            return Err(KeyStoreError::KeyNotFound(account_id.clone()));
        }

        Ok(keys)
    }

    /// Check if a credential file exists for an account.
    pub fn exists(&self, account_id: &AccountId) -> bool {
        self.key_file_path(account_id).exists() || self.multi_key_dir(account_id).is_dir()
    }
}

impl KeyStore for FileKeyStore {
    fn add(&self, account_id: &AccountId, secret_key: SecretKey) {
        // Note: KeyStore::add doesn't return errors, so we silently ignore failures
        // Use add_with_metadata() directly if you need error handling
        let _ = self.write_credential(account_id, &secret_key);
    }

    fn get(&self, account_id: &AccountId) -> Option<SecretKey> {
        // Try single-key format first
        if let Ok(key) = self.read_single_key(account_id) {
            return Some(key);
        }

        // Try multi-key format
        if let Ok(key) = self.read_multi_key(account_id) {
            return Some(key);
        }

        None
    }

    fn remove(&self, account_id: &AccountId) {
        // Remove single-key file
        let path = self.key_file_path(account_id);
        if path.exists() {
            let _ = fs::remove_file(path);
        }

        // Note: We don't remove multi-key directories, as that could
        // delete multiple keys. Users should manage that manually.
    }

    fn list(&self) -> Vec<AccountId> {
        let mut accounts = Vec::new();
        let network_path = self.network_path();

        if let Ok(entries) = fs::read_dir(&network_path) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();

                // Check for .json files (single-key format)
                if file_name_str.ends_with(".json") {
                    let account_str = file_name_str.trim_end_matches(".json");
                    if let Ok(account_id) = account_str.parse::<AccountId>() {
                        accounts.push(account_id);
                    }
                }

                // Check for directories (multi-key format)
                if entry.path().is_dir() {
                    if let Ok(account_id) = file_name_str.parse::<AccountId>() {
                        if !accounts.contains(&account_id) {
                            accounts.push(account_id);
                        }
                    }
                }
            }
        }

        accounts
    }
}

#[cfg(test)]
mod file_keystore_tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_keystore() -> (FileKeyStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let keystore = FileKeyStore::with_path(temp_dir.path(), "testnet").unwrap();
        (keystore, temp_dir)
    }

    #[test]
    fn test_file_keystore_add_get() {
        let (keystore, _temp_dir) = create_test_keystore();
        let account_id: AccountId = "alice.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        keystore.add(&account_id, secret);

        let retrieved = keystore.get(&account_id).unwrap();
        assert_eq!(retrieved.public_key(), public);
    }

    #[test]
    fn test_file_keystore_remove() {
        let (keystore, _temp_dir) = create_test_keystore();
        let account_id: AccountId = "bob.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();

        keystore.add(&account_id, secret);
        assert!(keystore.get(&account_id).is_some());

        keystore.remove(&account_id);
        assert!(keystore.get(&account_id).is_none());
    }

    #[test]
    fn test_file_keystore_list() {
        let (keystore, _temp_dir) = create_test_keystore();

        keystore.add(
            &"alice.testnet".parse().unwrap(),
            SecretKey::generate_ed25519(),
        );
        keystore.add(
            &"bob.testnet".parse().unwrap(),
            SecretKey::generate_ed25519(),
        );

        let accounts = keystore.list();
        assert_eq!(accounts.len(), 2);
        assert!(accounts.contains(&"alice.testnet".parse().unwrap()));
        assert!(accounts.contains(&"bob.testnet".parse().unwrap()));
    }

    #[test]
    fn test_file_keystore_legacy_format() {
        let (keystore, temp_dir) = create_test_keystore();
        let account_id: AccountId = "legacy.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        // Write in legacy format (secret_key instead of private_key)
        let network_path = temp_dir.path().join("testnet");
        fs::create_dir_all(&network_path).unwrap();

        let legacy_content = format!(
            r#"{{
                "account_id": "legacy.testnet",
                "public_key": "{}",
                "secret_key": "{}"
            }}"#,
            public, secret
        );

        let path = network_path.join("legacy.testnet.json");
        fs::write(&path, legacy_content).unwrap();

        // Should be able to read legacy format
        let retrieved = keystore.get(&account_id).unwrap();
        assert_eq!(retrieved.public_key(), public);
    }

    #[test]
    fn test_file_keystore_multi_key_format() {
        let (keystore, temp_dir) = create_test_keystore();
        let account_id: AccountId = "multi.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        // Create multi-key directory structure
        let account_dir = temp_dir.path().join("testnet").join("multi.testnet");
        fs::create_dir_all(&account_dir).unwrap();

        let key_file_name = format!("{}.json", public.to_string().replace(':', "_"));
        let content = format!(
            r#"{{
                "account_id": "multi.testnet",
                "public_key": "{}",
                "private_key": "{}"
            }}"#,
            public, secret
        );

        fs::write(account_dir.join(&key_file_name), content).unwrap();

        // Should be able to read from multi-key directory
        let retrieved = keystore.get(&account_id).unwrap();
        assert_eq!(retrieved.public_key(), public);
    }

    #[test]
    fn test_add_with_metadata() {
        let (keystore, temp_dir) = create_test_keystore();
        let account_id: AccountId = "seed.testnet".parse().unwrap();
        let secret = SecretKey::generate_ed25519();

        keystore
            .add_with_metadata(
                &account_id,
                &secret,
                Some("word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12"),
                Some("m/44'/397'/0'"),
            )
            .unwrap();

        // Verify the file was written with metadata
        let path = temp_dir.path().join("testnet").join("seed.testnet.json");
        let content = fs::read_to_string(&path).unwrap();
        let cred: NearCliCredential = serde_json::from_str(&content).unwrap();

        assert!(cred.master_seed_phrase.is_some());
        assert!(cred.seed_phrase_hd_path.is_some());
        assert_eq!(cred.seed_phrase_hd_path.as_deref(), Some("m/44'/397'/0'"));
    }

    #[test]
    fn test_get_all_keys() {
        let (keystore, temp_dir) = create_test_keystore();
        let account_id: AccountId = "multi.testnet".parse().unwrap();

        // Add single-key file
        let secret1 = SecretKey::generate_ed25519();
        keystore.add(&account_id, secret1.clone());

        // Add multi-key file
        let secret2 = SecretKey::generate_ed25519();
        let public2 = secret2.public_key();

        let account_dir = temp_dir.path().join("testnet").join("multi.testnet");
        fs::create_dir_all(&account_dir).unwrap();

        let key_file_name = format!("{}.json", public2.to_string().replace(':', "_"));
        let content = format!(
            r#"{{
                "public_key": "{}",
                "private_key": "{}"
            }}"#,
            public2, secret2
        );
        fs::write(account_dir.join(&key_file_name), content).unwrap();

        // Should retrieve both keys
        let keys = keystore.get_all_keys(&account_id).unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_exists() {
        let (keystore, temp_dir) = create_test_keystore();
        let account_id: AccountId = "exists.testnet".parse().unwrap();

        assert!(!keystore.exists(&account_id));

        // Create single-key file
        keystore.add(&account_id, SecretKey::generate_ed25519());
        assert!(keystore.exists(&account_id));

        // Remove and check multi-key format
        keystore.remove(&account_id);
        assert!(!keystore.exists(&account_id));

        // Create multi-key directory
        let account_dir = temp_dir.path().join("testnet").join("exists.testnet");
        fs::create_dir_all(&account_dir).unwrap();
        assert!(keystore.exists(&account_id));
    }
}
