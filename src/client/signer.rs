//! Signer trait and implementations.
//!
//! A `Signer` provides the ability to sign transactions using keys from a [`KeyStore`].

use std::sync::Arc;

use crate::error::SignerError;
use crate::types::{AccountId, PublicKey, Signature};

use super::keystore::KeyStore;

/// Trait for signing transactions.
///
/// A signer provides the ability to sign messages (transaction hashes) and
/// knows which account and public key it represents.
///
/// # Implementing Custom Signers
///
/// You can implement this trait for hardware wallets, cloud KMS, or other
/// signing backends:
///
/// ```rust,ignore
/// struct LedgerSigner { /* ... */ }
///
/// impl Signer for LedgerSigner {
///     fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
///         // Sign using Ledger hardware wallet
///     }
///     
///     fn public_key(&self) -> &PublicKey { /* ... */ }
///     fn account_id(&self) -> &AccountId { /* ... */ }
/// }
/// ```
pub trait Signer: Send + Sync {
    /// Sign a message hash (typically the SHA-256 hash of a serialized transaction).
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError>;

    /// Get the public key for this signer.
    fn public_key(&self) -> &PublicKey;

    /// Get the account ID for this signer.
    fn account_id(&self) -> &AccountId;
}

/// A signer that uses a KeyStore to retrieve keys for signing.
///
/// This is the standard signer implementation. It retrieves the secret key
/// from the keystore when signing is needed.
///
/// # Example
///
/// ```rust
/// use near_kit::{InMemoryKeyStore, KeyStoreSigner, KeyStore, SecretKey};
/// use std::sync::Arc;
///
/// // Create a keystore and add a key
/// let keystore = Arc::new(InMemoryKeyStore::new());
/// let secret = SecretKey::generate_ed25519();
/// let account_id = "alice.testnet".parse().unwrap();
/// keystore.add(&account_id, secret);
///
/// // Create a signer for that account
/// let signer = KeyStoreSigner::new(keystore, account_id).unwrap();
/// ```
pub struct KeyStoreSigner {
    keystore: Arc<dyn KeyStore>,
    account_id: AccountId,
    public_key: PublicKey,
}

impl KeyStoreSigner {
    /// Create a new signer for the given account using keys from the keystore.
    ///
    /// Returns an error if no key exists for the account.
    pub fn new(keystore: Arc<dyn KeyStore>, account_id: AccountId) -> Result<Self, SignerError> {
        let secret_key = keystore.get(&account_id).ok_or_else(|| {
            SignerError::SigningFailed(format!("No key found for account {}", account_id))
        })?;
        let public_key = secret_key.public_key();
        Ok(Self {
            keystore,
            account_id,
            public_key,
        })
    }

    /// Get the underlying keystore.
    pub fn keystore(&self) -> &Arc<dyn KeyStore> {
        &self.keystore
    }
}

impl Signer for KeyStoreSigner {
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        let secret_key = self.keystore.get(&self.account_id).ok_or_else(|| {
            SignerError::SigningFailed(format!("No key found for account {}", self.account_id))
        })?;
        Ok(secret_key.sign(message))
    }

    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }
}

impl Clone for KeyStoreSigner {
    fn clone(&self) -> Self {
        Self {
            keystore: self.keystore.clone(),
            account_id: self.account_id.clone(),
            public_key: self.public_key.clone(),
        }
    }
}

impl std::fmt::Debug for KeyStoreSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyStoreSigner")
            .field("account_id", &self.account_id)
            .field("public_key", &self.public_key)
            .finish()
    }
}

impl Signer for Arc<dyn Signer> {
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        (**self).sign(message)
    }

    fn public_key(&self) -> &PublicKey {
        (**self).public_key()
    }

    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }
}
