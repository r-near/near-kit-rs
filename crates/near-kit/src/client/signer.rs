//! Signer trait and implementations.
//!
//! A `Signer` knows which account it signs for and provides keys for signing.
//! The `key()` method returns a `SigningKey` that bundles together the public key
//! and signing capability, ensuring atomic key claiming for rotating signers.
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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::SignerError;
use crate::types::nep413::{self, SignMessageParams, SignedMessage};
use crate::types::{AccountId, PublicKey, SecretKey, Signature};

// ============================================================================
// Signer Trait
// ============================================================================

/// Trait for signing transactions.
///
/// A signer knows which account it signs for and provides keys for signing.
/// The `key()` method returns a [`SigningKey`] that bundles together the public
/// key and signing capability, ensuring atomic key claiming.
///
/// # Example Implementation
///
/// ```rust,ignore
/// use near_kit::{Signer, SigningKey, AccountId, SecretKey};
///
/// struct MyCustomSigner {
///     account_id: AccountId,
///     secret_key: SecretKey,
/// }
///
/// impl Signer for MyCustomSigner {
///     fn account_id(&self) -> &AccountId {
///         &self.account_id
///     }
///
///     fn key(&self) -> SigningKey {
///         SigningKey::new(self.secret_key.clone())
///     }
/// }
/// ```
pub trait Signer: Send + Sync {
    /// The account this signer signs for.
    fn account_id(&self) -> &AccountId;

    /// Get a key for signing.
    ///
    /// Returns a [`SigningKey`] that contains both the public key and the
    /// capability to sign with the corresponding private key.
    ///
    /// For single-key signers, this always returns the same key.
    /// For rotating signers, this atomically claims the next key in rotation.
    fn key(&self) -> SigningKey;
}

/// Implement `Signer` for `Arc<dyn Signer>` for convenience.
impl Signer for Arc<dyn Signer> {
    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn key(&self) -> SigningKey {
        (**self).key()
    }
}

// ============================================================================
// SigningKey
// ============================================================================

/// A key that can sign messages.
///
/// This bundles together a public key and the ability to sign with the
/// corresponding private key. For in-memory keys, signing is instant.
/// For hardware wallets or KMS, signing may involve async operations.
///
/// # Example
///
/// ```rust
/// use near_kit::{InMemorySigner, Signer};
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let signer = InMemorySigner::new("alice.testnet", "ed25519:...")?;
///
/// let key = signer.key();
/// println!("Public key: {}", key.public_key());
///
/// let signature = key.sign(b"message").await?;
/// # Ok(())
/// # }
/// ```
pub struct SigningKey {
    /// The public key.
    public_key: PublicKey,
    /// The signing backend.
    backend: Arc<dyn SigningBackend>,
}

impl SigningKey {
    /// Create a new signing key from a secret key.
    pub fn new(secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self {
            public_key,
            backend: Arc::new(SecretKeyBackend { secret_key }),
        }
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Sign a message.
    ///
    /// For in-memory keys, this returns immediately.
    /// For hardware wallets or KMS, this may involve user confirmation or
    /// network requests.
    pub async fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        self.backend.sign(message).await
    }

    /// Sign a NEP-413 message for off-chain authentication.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::{InMemorySigner, Signer, nep413};
    ///
    /// let signer = InMemorySigner::new("alice.testnet", "ed25519:...")?;
    /// let key = signer.key();
    ///
    /// let params = nep413::SignMessageParams {
    ///     message: "Login to MyApp".to_string(),
    ///     recipient: "myapp.com".to_string(),
    ///     nonce: nep413::generate_nonce(),
    ///     callback_url: None,
    ///     state: None,
    /// };
    ///
    /// let signed = key.sign_nep413(&signer.account_id(), &params).await?;
    /// ```
    pub async fn sign_nep413(
        &self,
        account_id: &AccountId,
        params: &SignMessageParams,
    ) -> Result<SignedMessage, SignerError> {
        let hash = nep413::serialize_message(params);
        let signature = self.sign(hash.as_bytes()).await?;

        Ok(SignedMessage {
            account_id: account_id.clone(),
            public_key: self.public_key.clone(),
            signature,
            state: params.state.clone(),
        })
    }
}

impl Clone for SigningKey {
    fn clone(&self) -> Self {
        Self {
            public_key: self.public_key.clone(),
            backend: self.backend.clone(),
        }
    }
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("public_key", &self.public_key)
            .finish()
    }
}

// ============================================================================
// SigningBackend (internal)
// ============================================================================

/// Internal trait for signing backends.
///
/// This allows different implementations (in-memory, hardware wallet, KMS)
/// to provide signing capability.
trait SigningBackend: Send + Sync {
    fn sign(
        &self,
        message: &[u8],
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignerError>> + Send + '_>>;
}

/// In-memory signing backend using a secret key.
struct SecretKeyBackend {
    secret_key: SecretKey,
}

impl SigningBackend for SecretKeyBackend {
    fn sign(
        &self,
        message: &[u8],
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignerError>> + Send + '_>> {
        let sig = self.secret_key.sign(message);
        Box::pin(async move { Ok(sig) })
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

    fn key(&self) -> SigningKey {
        SigningKey::new(self.secret_key.clone())
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

    fn key(&self) -> SigningKey {
        self.inner.key()
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

    fn key(&self) -> SigningKey {
        self.inner.key()
    }
}

// ============================================================================
// RotatingSigner
// ============================================================================

/// A signer that rotates through multiple keys for the same account.
///
/// This solves the nonce collision problem for high-throughput applications.
/// Each call to `key()` atomically claims the next key in round-robin order.
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
/// use near_kit::{RotatingSigner, SecretKey, Signer};
///
/// let keys = vec![
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
/// ];
///
/// let signer = RotatingSigner::new("bot.testnet", keys).unwrap();
///
/// // Each key() call atomically claims the next key in sequence
/// let key1 = signer.key();
/// let key2 = signer.key();
/// let key3 = signer.key();
/// // key4 wraps back to the first key
/// let key4 = signer.key();
/// ```
pub struct RotatingSigner {
    account_id: AccountId,
    keys: Vec<SecretKey>,
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
    pub fn public_keys(&self) -> Vec<PublicKey> {
        self.keys.iter().map(|sk| sk.public_key()).collect()
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

    fn key(&self) -> SigningKey {
        // Atomically claim the next key in rotation
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        SigningKey::new(self.keys[idx].clone())
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

        let key = signer.key();
        let message = b"test message";
        let signature = key.sign(message).await.unwrap();

        // Verify the key matches
        assert_eq!(key.public_key(), signer.public_key());
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

        let key = signer.key();
        let message = b"test message";
        let sig1 = key.sign(message).await.unwrap();
        let sig2 = key.sign(message).await.unwrap();

        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[tokio::test]
    async fn test_different_messages_different_signatures() {
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        let key = signer.key();
        let sig1 = key.sign(b"message 1").await.unwrap();
        let sig2 = key.sign(b"message 2").await.unwrap();

        assert_ne!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[tokio::test]
    async fn test_transaction_signing_with_signer_trait() {
        let secret_key = SecretKey::generate_ed25519();
        let signer = InMemorySigner::from_secret_key("alice.testnet".parse().unwrap(), secret_key);

        // Get a key for signing
        let key = signer.key();

        // Build a transaction
        let tx = Transaction::new(
            signer.account_id().clone(),
            key.public_key().clone(),
            1,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![Action::transfer(NearToken::from_near(1))],
        );

        // Sign using the key
        let tx_hash = tx.get_hash();
        let signature = key.sign(tx_hash.as_bytes()).await.unwrap();

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
        let key1 = signer.key();
        assert_eq!(key1.public_key(), &expected_public_keys[0]);

        let key2 = signer.key();
        assert_eq!(key2.public_key(), &expected_public_keys[1]);

        let key3 = signer.key();
        assert_eq!(key3.public_key(), &expected_public_keys[2]);

        // Wraps around
        let key4 = signer.key();
        assert_eq!(key4.public_key(), &expected_public_keys[0]);
    }

    #[tokio::test]
    async fn test_rotating_signer_atomic_key_claiming() {
        // Verify that key() atomically claims a key that can be used for signing
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let expected_pks: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new("bot.testnet", keys.clone()).unwrap();
        let message = b"test";

        // Claim first key and sign
        let key1 = signer.key();
        assert_eq!(key1.public_key(), &expected_pks[0]);
        let sig1 = key1.sign(message).await.unwrap();
        // Verify signature matches what the raw key would produce
        let expected_sig1 = keys[0].sign(message);
        assert_eq!(sig1.as_bytes(), expected_sig1.as_bytes());

        // Claim second key and sign
        let key2 = signer.key();
        assert_eq!(key2.public_key(), &expected_pks[1]);
        let sig2 = key2.sign(message).await.unwrap();
        let expected_sig2 = keys[1].sign(message);
        assert_eq!(sig2.as_bytes(), expected_sig2.as_bytes());

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

    #[test]
    fn test_signing_key_is_clone() {
        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
        )
        .unwrap();

        let key = signer.key();
        let key_clone = key.clone();

        assert_eq!(key.public_key(), key_clone.public_key());
    }
}
