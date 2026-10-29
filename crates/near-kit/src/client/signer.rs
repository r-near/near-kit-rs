//! Signers that load their key from the environment or the filesystem.
//!
//! The [`Signer`] trait, [`SigningKey`], and the in-memory and rotating
//! signers live in `near-kit-primitives`; these wrap an [`InMemorySigner`] and
//! report failures as [`Error`](crate::Error).

#[cfg(feature = "file-signer")]
use std::path::Path;

#[cfg(feature = "file-signer")]
use crate::types::TryIntoAccountId;
use crate::types::{AccountId, InMemorySigner, PublicKey, Signer, SigningKey};

// ============================================================================
// FileSigner
// ============================================================================

/// A signer that loads its key from `~/.near-credentials/{network}/{account}.json`.
///
/// Compatible with credentials created by near-cli and near-cli-rs.
///
/// Requires the opt-in `file-signer` feature and is not usable on
/// `wasm32-unknown-unknown`, which has no filesystem.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::signer::FileSigner;
///
/// // Load from ~/.near-credentials/testnet/alice.testnet.json
/// let signer = FileSigner::new("testnet", "alice.testnet").unwrap();
/// ```
#[cfg(feature = "file-signer")]
#[derive(Clone)]
pub struct FileSigner {
    inner: InMemorySigner,
}

/// Credential file format compatible with near-cli.
#[cfg(feature = "file-signer")]
#[derive(serde::Deserialize)]
struct CredentialFile {
    #[serde(alias = "secret_key")]
    private_key: String,
}

#[cfg(feature = "file-signer")]
impl FileSigner {
    /// Load credentials for an account from the standard NEAR credentials directory.
    ///
    /// Looks for the file at `~/.near-credentials/{network}/{account_id}.json`.
    ///
    /// # Arguments
    ///
    /// * `network` - Network name (e.g., "testnet", "mainnet")
    /// * `account_id` - The NEAR account ID
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The home directory cannot be determined
    /// - The credentials file doesn't exist
    /// - The file cannot be parsed
    pub fn new(
        network: impl AsRef<str>,
        account_id: impl TryIntoAccountId,
    ) -> Result<Self, crate::error::Error> {
        let account_id: AccountId = account_id.try_into_account_id()?;
        let home = dirs::home_dir().ok_or_else(|| {
            crate::error::Error::Config("Could not determine home directory".to_string())
        })?;
        let path = home
            .join(".near-credentials")
            .join(network.as_ref())
            .join(format!("{}.json", account_id));

        Self::from_file(&path, account_id)
    }

    /// Load credentials from a specific file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the credentials JSON file
    /// * `account_id` - The NEAR account ID
    pub fn from_file(
        path: impl AsRef<Path>,
        account_id: impl TryIntoAccountId,
    ) -> Result<Self, crate::error::Error> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            crate::error::Error::Config(format!(
                "Failed to read credentials file {}: {}",
                path.as_ref().display(),
                e
            ))
        })?;

        let cred: CredentialFile = serde_json::from_str(&content).map_err(|e| {
            crate::error::Error::Config(format!(
                "Failed to parse credentials file {}: {}",
                path.as_ref().display(),
                e
            ))
        })?;

        let inner = InMemorySigner::new(account_id, &cred.private_key)?;
        Ok(Self { inner })
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }

    /// Unwrap into the underlying [`InMemorySigner`].
    ///
    /// Use this to combine keys from several credential files into a
    /// [`RotatingSigner`](crate::signer::RotatingSigner):
    ///
    /// ```rust,no_run
    /// use near_kit::signer::{FileSigner, RotatingSigner};
    ///
    /// // Load keys from separate credential files for the same account
    /// let signers = vec![
    ///     FileSigner::from_file("keys/bot-key-0.json", "bot.testnet")?.into_inner(),
    ///     FileSigner::from_file("keys/bot-key-1.json", "bot.testnet")?.into_inner(),
    /// ];
    /// let rotating = RotatingSigner::from_signers(signers)?;
    /// # Ok::<(), near_kit::Error>(())
    /// ```
    pub fn into_inner(self) -> InMemorySigner {
        self.inner
    }
}

#[cfg(feature = "file-signer")]
impl std::fmt::Debug for FileSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSigner")
            .field("account_id", self.inner.account_id())
            .field("public_key", self.inner.public_key())
            .finish()
    }
}

#[cfg(feature = "file-signer")]
impl Signer for FileSigner {
    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn key(&self) -> SigningKey {
        self.inner.key()
    }

    fn public_key(&self) -> PublicKey {
        Signer::public_key(&self.inner)
    }
}

// ============================================================================
// EnvSigner
// ============================================================================

/// A signer that loads credentials from environment variables.
///
/// By default, reads from:
/// - `NEAR_ACCOUNT_ID` - The account ID
/// - `NEAR_PRIVATE_KEY` - The private key
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::signer::EnvSigner;
///
/// // With NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY set:
/// let signer = EnvSigner::new().unwrap();
/// ```
#[derive(Clone)]
pub struct EnvSigner {
    inner: InMemorySigner,
}

impl EnvSigner {
    /// Load from `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Either environment variable is not set
    /// - The values cannot be parsed
    pub fn new() -> Result<Self, crate::error::Error> {
        Self::from_env_vars("NEAR_ACCOUNT_ID", "NEAR_PRIVATE_KEY")
    }

    /// Load from custom environment variable names.
    ///
    /// # Arguments
    ///
    /// * `account_var` - Name of the environment variable containing the account ID
    /// * `key_var` - Name of the environment variable containing the private key
    pub fn from_env_vars(account_var: &str, key_var: &str) -> Result<Self, crate::error::Error> {
        let account_id = std::env::var(account_var).map_err(|_| {
            crate::error::Error::Config(format!("Environment variable {} not set", account_var))
        })?;

        let private_key = std::env::var(key_var).map_err(|_| {
            crate::error::Error::Config(format!("Environment variable {} not set", key_var))
        })?;

        let inner = InMemorySigner::new(account_id, &private_key)?;
        Ok(Self { inner })
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }

    /// Unwrap into the underlying [`InMemorySigner`].
    pub fn into_inner(self) -> InMemorySigner {
        self.inner
    }
}

impl std::fmt::Debug for EnvSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvSigner")
            .field("account_id", self.inner.account_id())
            .field("public_key", self.inner.public_key())
            .finish()
    }
}

impl Signer for EnvSigner {
    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn key(&self) -> SigningKey {
        self.inner.key()
    }

    fn public_key(&self) -> PublicKey {
        Signer::public_key(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_signer_missing_vars() {
        // This should fail because the env vars aren't set
        let result = EnvSigner::from_env_vars("NONEXISTENT_VAR_1", "NONEXISTENT_VAR_2");
        assert!(result.is_err());
    }
}
