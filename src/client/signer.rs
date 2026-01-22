//! Signer trait and implementations.

use std::sync::Arc;

use crate::error::SignerError;
use crate::types::{AccountId, PublicKey, SecretKey, Signature};

/// Trait for signing transactions.
pub trait Signer: Send + Sync {
    /// Sign a message hash.
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError>;

    /// Get the public key.
    fn public_key(&self) -> &PublicKey;

    /// Get the account ID (if known).
    fn account_id(&self) -> Option<&AccountId>;
}

/// Signer implementation using a secret key.
#[derive(Clone)]
pub struct SecretKeySigner {
    secret_key: SecretKey,
    public_key: PublicKey,
    account_id: Option<AccountId>,
}

impl SecretKeySigner {
    /// Create a new signer from a secret key.
    pub fn new(secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self {
            secret_key,
            public_key,
            account_id: None,
        }
    }

    /// Create a new signer with an associated account ID.
    pub fn with_account_id(secret_key: SecretKey, account_id: AccountId) -> Self {
        let public_key = secret_key.public_key();
        Self {
            secret_key,
            public_key,
            account_id: Some(account_id),
        }
    }

    /// Parse from a secret key string (e.g., "ed25519:...").
    pub fn from_secret_key(s: &str) -> Result<Self, crate::error::ParseKeyError> {
        let secret_key: SecretKey = s.parse()?;
        Ok(Self::new(secret_key))
    }

    /// Generate a new random signer.
    pub fn generate() -> Self {
        Self::new(SecretKey::generate_ed25519())
    }
}

impl Signer for SecretKeySigner {
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        Ok(self.secret_key.sign(message))
    }

    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    fn account_id(&self) -> Option<&AccountId> {
        self.account_id.as_ref()
    }
}

impl Signer for Arc<dyn Signer> {
    fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        (**self).sign(message)
    }

    fn public_key(&self) -> &PublicKey {
        (**self).public_key()
    }

    fn account_id(&self) -> Option<&AccountId> {
        (**self).account_id()
    }
}

impl std::fmt::Debug for SecretKeySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretKeySigner")
            .field("public_key", &self.public_key)
            .field("account_id", &self.account_id)
            .finish()
    }
}
