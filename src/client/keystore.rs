//! Key storage implementations.
//!
//! A `KeyStore` manages private keys for multiple accounts. Different implementations
//! support different storage backends:
//!
//! - [`InMemoryKeyStore`] - Ephemeral storage in RAM (testing, scripts)
//! - `FileKeyStore` - NEAR CLI compatible file storage (coming soon)
//! - `RotatingKeyStore` - Multiple keys per account for high concurrency (coming soon)

use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::{AccountId, SecretKey};

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
