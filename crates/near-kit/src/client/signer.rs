//! Signer trait and implementations.
//!
//! A `Signer` knows which account it signs for and can sign arbitrary messages.
//! The trait uses async signing to support remote signers (hardware wallets, cloud KMS, etc.).
//!
//! # Implementations
//!
//! - [`InMemorySigner`] - Single key stored in memory
//! - [`FileSigner`] - Key loaded from ~/.near-credentials
//! - [`EnvSigner`] - Key loaded from environment variables
//! - [`RotatingSigner`] - Multiple keys with round-robin rotation
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::{Near, InMemorySigner};
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let signer = InMemorySigner::new(
//!     "alice.testnet",
//!     "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"
//! )?;
//!
//! let near = Near::testnet()
//!     .signer(signer)
//!     .build();
//!
//! near.transfer("bob.testnet", "1 NEAR").await?;
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::error::SignerError;
use crate::types::nep413::{self, SignMessageParams, SignedMessage};
use crate::types::{AccountId, PublicKey, SecretKey, Signature};

// ============================================================================
// Signer Trait
// ============================================================================

/// Trait for signing transactions.
///
/// A signer knows which account it signs for and can sign arbitrary messages.
/// The `sign` method returns both the signature and the public key used,
/// which enables key rotation (where different calls may use different keys).
///
/// # Async Signing
///
/// The trait uses async signing to support:
/// - Hardware wallets (user confirmation)
/// - Cloud KMS (network requests)
/// - Remote signing services
///
/// # Example Implementation
///
/// ```rust,ignore
/// use near_kit::{Signer, SignerError, AccountId, PublicKey, Signature};
///
/// struct MyCustomSigner {
///     account_id: AccountId,
///     // ... your signing backend
/// }
///
/// impl Signer for MyCustomSigner {
///     fn account_id(&self) -> &AccountId {
///         &self.account_id
///     }
///
///     fn sign(&self, message: &[u8]) -> SignFuture<'_> {
///         let account_id = self.account_id.clone();
///         Box::pin(async move {
///             // Your signing logic here
///             todo!()
///         })
///     }
/// }
/// ```
pub trait Signer: Send + Sync {
    /// The account this signer signs for.
    fn account_id(&self) -> &AccountId;

    /// Get the public key that will be used for the next signing operation.
    ///
    /// For single-key signers, this always returns the same key.
    /// For rotating signers, this returns the next key in the rotation
    /// (the same one that will be used by the next `sign()` call).
    ///
    /// **Warning:** For `RotatingSigner`, there is no guarantee that this key
    /// will still be used by `sign()` if other threads/tasks call methods on
    /// the signer between your `public_key()` and `sign()` calls.
    /// Use [`claim_key`](Signer::claim_key) for atomic key claiming.
    fn public_key(&self) -> &PublicKey;

    /// Sign a message, returning the signature and the public key used.
    ///
    /// Returning the public key allows signers to use different keys for
    /// different transactions (e.g., key rotation for high-throughput bots).
    fn sign(&self, message: &[u8]) -> SignFuture<'_>;

    /// Atomically claim a key for exclusive use.
    ///
    /// This returns the public key that will be used AND a handle that will
    /// sign with that exact key. This solves the race condition in rotating
    /// signers where concurrent callers could interleave their `public_key()`
    /// and `sign()` calls.
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `PublicKey` - The claimed key's public key
    /// - `Box<dyn ClaimedKey>` - A handle to sign with that specific key
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Claim a key atomically
    /// let (public_key, claimed) = signer.claim_key();
    ///
    /// // Build transaction with the claimed key
    /// let tx = Transaction::new(signer_id, public_key, nonce, ...);
    ///
    /// // Sign with the same key (guaranteed, even with concurrent access)
    /// let signature = claimed.sign(tx.get_hash().as_bytes());
    /// ```
    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>);

    /// Sign a NEP-413 message for off-chain authentication.
    ///
    /// The default implementation serializes the message, hashes it, and signs
    /// using [`sign()`](Signer::sign). Hardware wallet implementations may override
    /// this to call device-specific NEP-413 signing functions.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::{InMemorySigner, Signer, nep413};
    ///
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let signer = InMemorySigner::new("alice.testnet", "ed25519:...")?;
    ///
    /// let params = nep413::SignMessageParams {
    ///     message: "Login to MyApp".to_string(),
    ///     recipient: "myapp.com".to_string(),
    ///     nonce: nep413::generate_nonce(),
    ///     callback_url: None,
    ///     state: None,
    /// };
    ///
    /// let signed = signer.sign_nep413(&params).await?;
    /// # Ok(())
    /// # }
    /// ```
    fn sign_nep413<'a>(&'a self, params: &'a SignMessageParams) -> Nep413SignFuture<'a> {
        Box::pin(async move {
            let hash = nep413::serialize_message(params);
            let (signature, public_key) = self.sign(hash.as_bytes()).await?;

            Ok(SignedMessage {
                account_id: self.account_id().clone(),
                public_key,
                signature,
                state: params.state.clone(),
            })
        })
    }
}

/// A claimed key handle for signing.
///
/// This represents an atomically claimed key from a signer.
/// The `sign` method is guaranteed to use the same key that was
/// returned from [`Signer::claim_key`].
pub trait ClaimedKey {
    /// Sign a message with the claimed key.
    fn sign(&self, message: &[u8]) -> Signature;
}

/// A claimed key backed by a secret key.
struct SecretKeyClaimedKey {
    secret_key: SecretKey,
}

impl ClaimedKey for SecretKeyClaimedKey {
    fn sign(&self, message: &[u8]) -> Signature {
        self.secret_key.sign(message)
    }
}

/// Future type returned by [`Signer::sign`].
pub type SignFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(Signature, PublicKey), SignerError>> + Send + 'a>>;

/// Future type returned by [`Signer::sign_nep413`].
pub type Nep413SignFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SignedMessage, SignerError>> + Send + 'a>>;

/// Implement `Signer` for `Arc<dyn Signer>` for convenience.
impl Signer for Arc<dyn Signer> {
    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn public_key(&self) -> &PublicKey {
        (**self).public_key()
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        (**self).sign(message)
    }

    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>) {
        (**self).claim_key()
    }

    fn sign_nep413<'a>(&'a self, params: &'a SignMessageParams) -> Nep413SignFuture<'a> {
        (**self).sign_nep413(params)
    }
}

// ============================================================================
// InMemorySigner
// ============================================================================

/// A signer with a single key stored in memory.
///
/// This is the simplest signer implementation, suitable for scripts,
/// bots, and testing.
///
/// # Example
///
/// ```rust
/// use near_kit::InMemorySigner;
///
/// let signer = InMemorySigner::new(
///     "alice.testnet",
///     "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"
/// ).unwrap();
/// ```
#[derive(Clone)]
pub struct InMemorySigner {
    account_id: AccountId,
    secret_key: SecretKey,
    public_key: PublicKey,
}

impl InMemorySigner {
    /// Create a new signer with an account ID and secret key.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The NEAR account ID (e.g., "alice.testnet")
    /// * `secret_key` - The secret key in string format (e.g., "ed25519:...")
    ///
    /// # Errors
    ///
    /// Returns an error if the account ID or secret key cannot be parsed.
    pub fn new(
        account_id: impl AsRef<str>,
        secret_key: impl AsRef<str>,
    ) -> Result<Self, crate::error::Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let secret_key: SecretKey = secret_key.as_ref().parse()?;
        let public_key = secret_key.public_key();

        Ok(Self {
            account_id,
            secret_key,
            public_key,
        })
    }

    /// Create a signer from a SecretKey directly.
    pub fn from_secret_key(account_id: AccountId, secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self {
            account_id,
            secret_key,
            public_key,
        }
    }

    /// Create a signer from a BIP-39 seed phrase.
    ///
    /// Uses SLIP-10 derivation with the default NEAR HD path (`m/44'/397'/0'`).
    ///
    /// # Arguments
    ///
    /// * `account_id` - The NEAR account ID (e.g., "alice.testnet")
    /// * `phrase` - BIP-39 mnemonic phrase (12, 15, 18, 21, or 24 words)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::InMemorySigner;
    ///
    /// let signer = InMemorySigner::from_seed_phrase(
    ///     "alice.testnet",
    ///     "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    /// ).unwrap();
    /// ```
    pub fn from_seed_phrase(
        account_id: impl AsRef<str>,
        phrase: impl AsRef<str>,
    ) -> Result<Self, crate::error::Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let secret_key = SecretKey::from_seed_phrase(phrase)?;
        Ok(Self::from_secret_key(account_id, secret_key))
    }

    /// Create a signer from a BIP-39 seed phrase with custom HD path.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The NEAR account ID
    /// * `phrase` - BIP-39 mnemonic phrase
    /// * `hd_path` - BIP-32 derivation path (e.g., `"m/44'/397'/0'"`)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::InMemorySigner;
    ///
    /// let signer = InMemorySigner::from_seed_phrase_with_path(
    ///     "alice.testnet",
    ///     "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    ///     "m/44'/397'/1'"
    /// ).unwrap();
    /// ```
    pub fn from_seed_phrase_with_path(
        account_id: impl AsRef<str>,
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
    ) -> Result<Self, crate::error::Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let secret_key = SecretKey::from_seed_phrase_with_path(phrase, hd_path)?;
        Ok(Self::from_secret_key(account_id, secret_key))
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

impl std::fmt::Debug for InMemorySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemorySigner")
            .field("account_id", &self.account_id)
            .field("public_key", &self.public_key)
            .finish()
    }
}

impl Signer for InMemorySigner {
    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        let signature = self.secret_key.sign(message);
        let public_key = self.public_key.clone();
        Box::pin(async move { Ok((signature, public_key)) })
    }

    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>) {
        (
            self.public_key.clone(),
            Box::new(SecretKeyClaimedKey {
                secret_key: self.secret_key.clone(),
            }),
        )
    }
}

// ============================================================================
// FileSigner
// ============================================================================

/// A signer that loads its key from `~/.near-credentials/{network}/{account}.json`.
///
/// Compatible with credentials created by near-cli and near-cli-rs.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::FileSigner;
///
/// // Load from ~/.near-credentials/testnet/alice.testnet.json
/// let signer = FileSigner::new("testnet", "alice.testnet").unwrap();
/// ```
#[derive(Clone)]
pub struct FileSigner {
    inner: InMemorySigner,
}

/// Credential file format compatible with near-cli.
#[derive(serde::Deserialize)]
struct CredentialFile {
    #[serde(alias = "secret_key")]
    private_key: String,
}

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
        account_id: impl AsRef<str>,
    ) -> Result<Self, crate::error::Error> {
        let home = dirs::home_dir().ok_or_else(|| {
            crate::error::Error::Config("Could not determine home directory".to_string())
        })?;
        let path = home
            .join(".near-credentials")
            .join(network.as_ref())
            .join(format!("{}.json", account_id.as_ref()));

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
        account_id: impl AsRef<str>,
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
}

impl std::fmt::Debug for FileSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSigner")
            .field("account_id", &self.inner.account_id)
            .field("public_key", &self.inner.public_key)
            .finish()
    }
}

impl Signer for FileSigner {
    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        self.inner.sign(message)
    }

    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>) {
        self.inner.claim_key()
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
/// use near_kit::EnvSigner;
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

        let inner = InMemorySigner::new(&account_id, &private_key)?;
        Ok(Self { inner })
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }
}

impl std::fmt::Debug for EnvSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvSigner")
            .field("account_id", &self.inner.account_id)
            .field("public_key", &self.inner.public_key)
            .finish()
    }
}

impl Signer for EnvSigner {
    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn public_key(&self) -> &PublicKey {
        self.inner.public_key()
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        self.inner.sign(message)
    }

    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>) {
        self.inner.claim_key()
    }
}

// ============================================================================
// RotatingSigner
// ============================================================================

/// A signer that rotates through multiple keys for the same account.
///
/// This solves the nonce collision problem for high-throughput applications.
/// Each call to `sign()` uses the next key in round-robin order.
///
/// # Use Case
///
/// NEAR uses per-key nonces. When sending concurrent transactions with a single key,
/// they can collide on nonce values. By rotating through multiple keys, each
/// concurrent transaction uses a different key with its own nonce sequence.
///
/// # Example
///
/// ```rust
/// use near_kit::{RotatingSigner, SecretKey};
///
/// let keys = vec![
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
/// ];
///
/// let signer = RotatingSigner::new("bot.testnet", keys).unwrap();
///
/// // Each sign() call uses the next key in sequence
/// ```
pub struct RotatingSigner {
    account_id: AccountId,
    keys: Vec<(SecretKey, PublicKey)>,
    counter: AtomicUsize,
}

impl RotatingSigner {
    /// Create a rotating signer with multiple keys.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The NEAR account ID
    /// * `keys` - Vector of secret keys (must not be empty)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The account ID cannot be parsed
    /// - The keys vector is empty
    pub fn new(
        account_id: impl AsRef<str>,
        keys: Vec<SecretKey>,
    ) -> Result<Self, crate::error::Error> {
        if keys.is_empty() {
            return Err(crate::error::Error::Config(
                "RotatingSigner requires at least one key".to_string(),
            ));
        }

        let account_id: AccountId = account_id.as_ref().parse()?;
        let keys: Vec<_> = keys
            .into_iter()
            .map(|sk| {
                let pk = sk.public_key();
                (sk, pk)
            })
            .collect();

        Ok(Self {
            account_id,
            keys,
            counter: AtomicUsize::new(0),
        })
    }

    /// Create a rotating signer from key strings.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The NEAR account ID
    /// * `keys` - Slice of secret keys in string format (e.g., "ed25519:...")
    pub fn from_key_strings(
        account_id: impl AsRef<str>,
        keys: &[impl AsRef<str>],
    ) -> Result<Self, crate::error::Error> {
        let parsed_keys: Result<Vec<SecretKey>, _> =
            keys.iter().map(|k| k.as_ref().parse()).collect();
        Self::new(account_id, parsed_keys?)
    }

    /// Get the number of keys in rotation.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Get all public keys.
    pub fn public_keys(&self) -> Vec<&PublicKey> {
        self.keys.iter().map(|(_, pk)| pk).collect()
    }
}

impl std::fmt::Debug for RotatingSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotatingSigner")
            .field("account_id", &self.account_id)
            .field("key_count", &self.keys.len())
            .field("counter", &self.counter.load(Ordering::Relaxed))
            .finish()
    }
}

impl Signer for RotatingSigner {
    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn public_key(&self) -> &PublicKey {
        // Return the key that will be used by the next sign() call
        // (load returns current value, fetch_add in sign() returns value before incrementing)
        //
        // WARNING: This is racy for concurrent use! Use claim_key() instead.
        let idx = self.counter.load(Ordering::Relaxed) % self.keys.len();
        &self.keys[idx].1
    }

    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        // Round-robin key selection (fetch_add returns value BEFORE incrementing)
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        let (secret_key, public_key) = &self.keys[idx];

        let signature = secret_key.sign(message);
        let public_key = public_key.clone();

        Box::pin(async move { Ok((signature, public_key)) })
    }

    fn claim_key(&self) -> (PublicKey, Box<dyn ClaimedKey + Send>) {
        // Atomically claim a key - this is the safe way to get a consistent key
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        let (secret_key, public_key) = &self.keys[idx];

        (
            public_key.clone(),
            Box::new(SecretKeyClaimedKey {
                secret_key: secret_key.clone(),
            }),
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, CryptoHash, NearToken, Transaction};

    #[tokio::test]
    async fn test_in_memory_signer() {
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        assert_eq!(signer.account_id().as_str(), "alice.testnet");

        let message = b"test message";
        let (signature, public_key) = signer.sign(message).await.unwrap();

        // Verify the signature matches the public key
        assert_eq!(&public_key, signer.public_key());
        assert!(!signature.as_bytes().is_empty());
    }

    #[tokio::test]
    async fn test_signature_consistency() {
        // Same message should produce the same signature (Ed25519 is deterministic)
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        let message = b"test message";
        let (sig1, pk1) = signer.sign(message).await.unwrap();
        let (sig2, pk2) = signer.sign(message).await.unwrap();

        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
        assert_eq!(pk1, pk2);
    }

    #[tokio::test]
    async fn test_different_messages_different_signatures() {
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        let (sig1, _) = signer.sign(b"message 1").await.unwrap();
        let (sig2, _) = signer.sign(b"message 2").await.unwrap();

        assert_ne!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[tokio::test]
    async fn test_transaction_signing_with_signer_trait() {
        let secret_key = SecretKey::generate_ed25519();
        let signer = InMemorySigner::from_secret_key("alice.testnet".parse().unwrap(), secret_key);

        // Build a transaction
        let tx = Transaction::new(
            signer.account_id().clone(),
            signer.public_key().clone(),
            1,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![Action::transfer(NearToken::from_near(1))],
        );

        // Sign using the Signer trait (async)
        let tx_hash = tx.get_hash();
        let (signature, returned_pk) = signer.sign(tx_hash.as_bytes()).await.unwrap();

        // Verify the returned public key matches
        assert_eq!(&returned_pk, signer.public_key());

        // Verify signature is 64 bytes (Ed25519)
        assert_eq!(signature.as_bytes().len(), 64);

        // Create signed transaction
        let signed_tx = crate::types::SignedTransaction {
            transaction: tx,
            signature,
        };

        // Verify serialization works
        let bytes = signed_tx.to_bytes();
        assert!(!bytes.is_empty());

        // Verify base64 encoding works
        let base64 = signed_tx.to_base64();
        assert!(!base64.is_empty());
    }

    #[tokio::test]
    async fn test_rotating_signer() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new("bot.testnet", keys).unwrap();

        // Verify round-robin rotation
        let message = b"test";

        let (_, pk1) = signer.sign(message).await.unwrap();
        assert_eq!(pk1, expected_public_keys[0]);

        let (_, pk2) = signer.sign(message).await.unwrap();
        assert_eq!(pk2, expected_public_keys[1]);

        let (_, pk3) = signer.sign(message).await.unwrap();
        assert_eq!(pk3, expected_public_keys[2]);

        // Wraps around
        let (_, pk4) = signer.sign(message).await.unwrap();
        assert_eq!(pk4, expected_public_keys[0]);
    }

    #[tokio::test]
    async fn test_rotating_signer_public_key_sign_consistency() {
        // Verify that public_key() returns the key that sign() will use
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new("bot.testnet", keys).unwrap();
        let message = b"test";

        // First iteration: public_key should match what sign returns
        let pk_before_sign = signer.public_key().clone();
        let (_, pk_from_sign) = signer.sign(message).await.unwrap();
        assert_eq!(pk_before_sign, pk_from_sign);
        assert_eq!(pk_from_sign, expected_public_keys[0]);

        // Second iteration
        let pk_before_sign = signer.public_key().clone();
        let (_, pk_from_sign) = signer.sign(message).await.unwrap();
        assert_eq!(pk_before_sign, pk_from_sign);
        assert_eq!(pk_from_sign, expected_public_keys[1]);

        // Third iteration (wraps to first key)
        let pk_before_sign = signer.public_key().clone();
        let (_, pk_from_sign) = signer.sign(message).await.unwrap();
        assert_eq!(pk_before_sign, pk_from_sign);
        assert_eq!(pk_from_sign, expected_public_keys[0]);
    }

    #[tokio::test]
    async fn test_rotating_signer_each_key_signs_correctly() {
        // Verify each key in rotation produces valid signatures
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];

        let signer = RotatingSigner::new("bot.testnet", keys.clone()).unwrap();
        let message = b"test message";

        // Sign with first key
        let (sig1, pk1) = signer.sign(message).await.unwrap();
        // Verify it matches what the raw key would produce
        let expected_sig1 = keys[0].sign(message);
        assert_eq!(sig1.as_bytes(), expected_sig1.as_bytes());
        assert_eq!(pk1, keys[0].public_key());

        // Sign with second key
        let (sig2, pk2) = signer.sign(message).await.unwrap();
        let expected_sig2 = keys[1].sign(message);
        assert_eq!(sig2.as_bytes(), expected_sig2.as_bytes());
        assert_eq!(pk2, keys[1].public_key());

        // Different keys produce different signatures
        assert_ne!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_rotating_signer_empty_keys() {
        let result = RotatingSigner::new("bot.testnet", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_env_signer_missing_vars() {
        // This should fail because the env vars aren't set
        let result = EnvSigner::from_env_vars("NONEXISTENT_VAR_1", "NONEXISTENT_VAR_2");
        assert!(result.is_err());
    }

    #[test]
    fn test_signer_from_secret_key() {
        let secret = SecretKey::generate_ed25519();
        let expected_pk = secret.public_key();

        let signer = InMemorySigner::from_secret_key("alice.testnet".parse().unwrap(), secret);

        assert_eq!(signer.account_id().as_str(), "alice.testnet");
        assert_eq!(signer.public_key(), &expected_pk);
    }

    #[test]
    fn test_rotating_signer_key_count() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];

        let signer = RotatingSigner::new("bot.testnet", keys).unwrap();

        assert_eq!(signer.key_count(), 3);
        assert_eq!(signer.public_keys().len(), 3);
    }

    #[test]
    fn test_rotating_signer_from_key_strings() {
        let keys = [
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
            "ed25519:4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi5kL6YJt9Z6iLqMBkVfqDH2Zj8bxqXTdMkNmvPcAD8LqCZ",
        ];

        let signer = RotatingSigner::from_key_strings("bot.testnet", &keys).unwrap();

        assert_eq!(signer.key_count(), 2);
        assert_eq!(signer.account_id().as_str(), "bot.testnet");
    }

    #[test]
    fn test_in_memory_signer_debug_hides_secret() {
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        let debug_str = format!("{:?}", signer);

        // Should show account_id and public_key but NOT the secret key
        assert!(debug_str.contains("alice.testnet"));
        assert!(debug_str.contains("public_key"));
        assert!(!debug_str.contains("secret_key"));
        assert!(!debug_str.contains("3D4YudUahN1nawWogh"));
    }
}
