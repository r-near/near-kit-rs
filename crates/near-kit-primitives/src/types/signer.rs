//! Signer trait and implementations.
//!
//! A `Signer` knows which account it signs for and provides keys for signing.
//! The `key()` method returns a `SigningKey` that bundles together the public key
//! and signing capability, ensuring atomic key claiming for rotating signers.
//! Custom hardware-wallet, HSM, and KMS integrations can implement
//! [`SigningBackend`] and wrap it with [`SigningKey::from_backend`].
//!
//! # Implementations
//!
//! - [`InMemorySigner`] - Single key stored in memory
//! - [`RotatingSigner`] - Multiple keys with round-robin rotation
//!
//! `near-kit` adds signers that load keys from the environment, the
//! `~/.near-credentials` directory, or the system keyring.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::SignerError;
use crate::types::nep413::{self, SignMessageParams, SignedMessage};
use crate::types::{AccountId, KeyType, PublicKey, SecretKey, Signature};

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
/// ```rust
/// use near_kit_primitives::AccountId;
/// use near_kit_primitives::signer::{PublicKey, SecretKey, Signer, SigningKey};
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
///
///     fn public_key(&self) -> PublicKey {
///         self.secret_key.public_key()
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

    /// Get the public key that the next call to [`key()`](Self::key) would claim.
    ///
    /// This method must be side-effect-free: it must not claim a key, advance
    /// rotation, prompt the user, or perform network I/O.
    fn public_key(&self) -> PublicKey;
}

/// Implement `Signer` for `Arc<T>` where `T: Signer`.
///
/// This covers `Arc<dyn Signer>` (since `dyn Signer: Signer`) as well as
/// concrete types like `Arc<InMemorySigner>`, `Arc<RotatingSigner>`, etc.
/// without requiring a cast to `Arc<dyn Signer>` first.
impl<T: Signer + ?Sized> Signer for Arc<T> {
    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn key(&self) -> SigningKey {
        (**self).key()
    }

    fn public_key(&self) -> PublicKey {
        (**self).public_key()
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
/// use near_kit_primitives::account;
/// use near_kit_primitives::signer::{InMemorySigner, Signer};
///
/// # async fn example() -> Result<(), near_kit_primitives::signer::SignerError> {
/// let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?);
///
/// let key = signer.key();
/// println!("Public key: {}", key.public_key());
///
/// let signature = key.sign(b"message").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SigningKey {
    /// The public key.
    public_key: PublicKey,
    /// The signing backend.
    backend: Arc<dyn ErasedSigningBackend>,
}

impl SigningKey {
    /// Create a new signing key from a secret key.
    pub fn new(secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self::from_backend(public_key, SecretKeyBackend { secret_key })
    }

    /// Create a signing key backed by a custom asynchronous signer.
    ///
    /// `public_key` is the key claimed by this handle. `backend` must produce
    /// signatures for that same key. This supports external signers such as
    /// hardware wallets, HSMs, and cloud KMS services without exposing a
    /// private key to near-kit.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::signer::{
    ///     SecretKey, Signature, SignerError, SigningBackend, SigningKey,
    /// };
    ///
    /// struct CustomBackend(SecretKey);
    ///
    /// impl SigningBackend for CustomBackend {
    ///     async fn sign(
    ///         &self,
    ///         message: &[u8],
    ///     ) -> Result<Signature, SignerError> {
    ///         Ok(self.0.sign(message))
    ///     }
    /// }
    ///
    /// let secret_key = SecretKey::generate_ed25519();
    /// let public_key = secret_key.public_key();
    /// let key = SigningKey::from_backend(public_key.clone(), CustomBackend(secret_key));
    /// assert_eq!(key.public_key(), &public_key);
    /// ```
    pub fn from_backend<B>(public_key: PublicKey, backend: B) -> Self
    where
        B: SigningBackend + 'static,
    {
        Self {
            public_key,
            backend: Arc::new(BackendAdapter(backend)),
        }
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Sign a message.
    ///
    /// The bytes are signed as nearcore expects: Ed25519 and ML-DSA-65 sign
    /// them as-is, while Secp256k1 treats them as a 32-byte digest and signs
    /// it without hashing again (see [`SecretKey::sign`]). An in-memory
    /// Secp256k1 key returns [`SignerError::SigningFailed`] for any other
    /// length.
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
    /// ```rust
    /// use near_kit_primitives::signer::{InMemorySigner, SecretKey, Signer};
    /// use near_kit_primitives::standards::nep413;
    ///
    /// # async fn example() -> Result<(), near_kit_primitives::signer::SignerError> {
    /// let signer = InMemorySigner::implicit(SecretKey::generate_ed25519())?;
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
    /// let signed = key.sign_nep413(signer.account_id(), &params).await?;
    /// assert!(nep413::verify_signature(&signed, &params, nep413::NonceValidation::default()));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sign_nep413(
        &self,
        account_id: &AccountId,
        params: &SignMessageParams,
    ) -> Result<SignedMessage, SignerError> {
        let hash = nep413::hash_message(params);
        let signature = self.sign(hash.as_bytes()).await?;

        Ok(SignedMessage {
            account_id: account_id.clone(),
            public_key: self.public_key.clone(),
            signature,
            state: params.state.clone(),
        })
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
// SigningBackend
// ============================================================================

/// Asynchronous backend for a [`SigningKey`].
///
/// Implement this trait to integrate a hardware wallet, HSM, cloud KMS, or
/// another service that signs asynchronously. Pass the implementation and its
/// corresponding public key to [`SigningKey::from_backend`].
pub trait SigningBackend: crate::platform::MaybeSend + crate::platform::MaybeSync {
    /// Sign `message` with the private key corresponding to the public key
    /// supplied to [`SigningKey::from_backend`].
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn sign<'a>(
        &'a self,
        message: &'a [u8],
    ) -> impl Future<Output = Result<Signature, SignerError>> + Send + 'a;

    /// Sign `message` with the private key corresponding to the public key
    /// supplied to [`SigningKey::from_backend`].
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn sign<'a>(
        &'a self,
        message: &'a [u8],
    ) -> impl Future<Output = Result<Signature, SignerError>> + 'a;
}

/// Object-safe adapter kept private so public backends can use `async fn`
/// without exposing a boxed-future type in the API.
trait ErasedSigningBackend: crate::platform::MaybeSend + crate::platform::MaybeSync {
    fn sign<'a>(
        &'a self,
        message: &'a [u8],
    ) -> crate::platform::BoxFuture<'a, Result<Signature, SignerError>>;
}

struct BackendAdapter<B>(B);

impl<B: SigningBackend> ErasedSigningBackend for BackendAdapter<B> {
    fn sign<'a>(
        &'a self,
        message: &'a [u8],
    ) -> crate::platform::BoxFuture<'a, Result<Signature, SignerError>> {
        Box::pin(self.0.sign(message))
    }
}

/// In-memory signing backend using a secret key.
struct SecretKeyBackend {
    secret_key: SecretKey,
}

impl SigningBackend for SecretKeyBackend {
    async fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        // `SecretKey::sign` panics on this (as nearcore does); surface it as an
        // error instead since the caller already expects a `Result`.
        if self.secret_key.key_type() == KeyType::Secp256k1 && message.len() != 32 {
            return Err(SignerError::SigningFailed(format!(
                "secp256k1 signing expects a 32-byte digest, got {} bytes",
                message.len()
            )));
        }
        Ok(self.secret_key.sign(message))
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
/// use near_kit_primitives::account;
/// use near_kit_primitives::signer::InMemorySigner;
///
/// let signer = InMemorySigner::new(
///     account!("alice.testnet"),
///     "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse()?,
/// );
/// # Ok::<(), near_kit_primitives::signer::ParseKeyError>(())
/// ```
#[derive(Clone)]
pub struct InMemorySigner {
    account_id: AccountId,
    secret_key: SecretKey,
    public_key: PublicKey,
}

impl InMemorySigner {
    /// A signer for `account_id` with `secret_key`.
    ///
    /// Parse a key string with `"ed25519:...".parse::<SecretKey>()?`, and
    /// derive one from a seed phrase with [`SecretKey::from_seed_phrase`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::account;
    /// use near_kit_primitives::signer::{InMemorySigner, SecretKey, Signer};
    ///
    /// let account_id = account!("alice.testnet");
    /// let signer = InMemorySigner::new(&account_id, SecretKey::generate_ed25519());
    /// assert_eq!(signer.account_id(), &account_id);
    /// ```
    pub fn new(account_id: impl Into<AccountId>, secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self {
            account_id: account_id.into(),
            secret_key,
            public_key,
        }
    }

    /// A signer for the implicit account of an Ed25519 `secret_key`.
    ///
    /// The account ID is the hex-encoded Ed25519 public key bytes (64 characters).
    ///
    /// # Errors
    ///
    /// Returns [`SignerError::ImplicitAccountRequiresEd25519`] if the secret
    /// key is not Ed25519.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::signer::{InMemorySigner, SecretKey, Signer};
    ///
    /// let signer = InMemorySigner::implicit(SecretKey::generate_ed25519())?;
    /// assert_eq!(signer.account_id().as_str().len(), 64);
    /// # Ok::<(), near_kit_primitives::signer::SignerError>(())
    /// ```
    pub fn implicit(secret_key: SecretKey) -> Result<Self, SignerError> {
        let public_key = secret_key.public_key();
        let pk_bytes = public_key
            .as_ed25519_bytes()
            .ok_or(SignerError::ImplicitAccountRequiresEd25519)?;
        let account_id: AccountId = hex::encode(pk_bytes)
            .parse()
            .expect("hex-encoded Ed25519 public key is a valid account ID");
        Ok(Self {
            account_id,
            secret_key,
            public_key,
        })
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

    fn public_key(&self) -> PublicKey {
        self.public_key.clone()
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
/// # Loading Keys from Other Sources
///
/// Keys can be loaded from any storage backend (file, keyring, env) via
/// [`from_signers()`](Self::from_signers):
///
/// ```rust
/// use near_kit_primitives::account;
/// # use near_kit_primitives::signer::{InMemorySigner, RotatingSigner, SecretKey};
/// # let (key_0, key_1) = (SecretKey::generate_ed25519(), SecretKey::generate_ed25519());
/// let rotating = RotatingSigner::from_signers(vec![
///     InMemorySigner::new(account!("bot.testnet"), key_0),
///     InMemorySigner::new(account!("bot.testnet"), key_1),
/// ])?;
/// # Ok::<(), near_kit_primitives::signer::SignerError>(())
/// ```
///
/// # Sequential Sends
///
/// Use [`into_per_key_signers()`](Self::into_per_key_signers) to split into
/// per-key [`InMemorySigner`] instances for building sequential send queues.
/// See the `sequential_sends` example.
///
/// # Example
///
/// ```rust
/// use near_kit_primitives::account;
/// use near_kit_primitives::signer::{RotatingSigner, SecretKey, Signer};
///
/// let keys = vec![
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
///     SecretKey::generate_ed25519(),
/// ];
///
/// let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();
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
        account_id: impl Into<AccountId>,
        keys: Vec<SecretKey>,
    ) -> Result<Self, SignerError> {
        if keys.is_empty() {
            return Err(SignerError::NoKeys);
        }

        let account_id: AccountId = account_id.into();

        Ok(Self {
            account_id,
            keys,
            counter: AtomicUsize::new(0),
        })
    }

    /// Create a rotating signer from [`InMemorySigner`] instances.
    ///
    /// This accepts signers loaded from any source (near-kit's
    /// `signer::from_credentials_file`, `from_keyring`, `from_env`, ...). All
    /// signers must share the same account ID.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::account;
    /// use near_kit_primitives::signer::{InMemorySigner, RotatingSigner, SecretKey};
    ///
    /// // Keys for the same account, loaded from wherever you keep them
    /// let signers = vec![
    ///     InMemorySigner::new(account!("bot.testnet"), SecretKey::generate_ed25519()),
    ///     InMemorySigner::new(account!("bot.testnet"), SecretKey::generate_ed25519()),
    /// ];
    /// let rotating = RotatingSigner::from_signers(signers)?;
    /// # Ok::<(), near_kit_primitives::signer::SignerError>(())
    /// ```
    pub fn from_signers(signers: Vec<InMemorySigner>) -> Result<Self, SignerError> {
        if signers.is_empty() {
            return Err(SignerError::NoKeys);
        }

        let account_id = signers[0].account_id().clone();
        for signer in &signers[1..] {
            if signer.account_id() != &account_id {
                return Err(SignerError::MixedAccounts {
                    expected: account_id,
                    found: signer.account_id().clone(),
                });
            }
        }

        let keys = signers.into_iter().map(|s| s.secret_key).collect();
        Ok(Self {
            account_id,
            keys,
            counter: AtomicUsize::new(0),
        })
    }

    /// Get all public keys.
    pub fn public_keys(&self) -> Vec<PublicKey> {
        self.keys.iter().map(|sk| sk.public_key()).collect()
    }

    /// Get all signing keys without advancing the rotation counter.
    ///
    /// Useful for building per-key data structures like sequential send queues.
    /// Unlike [`key()`](Signer::key), calling this does not affect the rotation.
    pub fn signing_keys(&self) -> Vec<SigningKey> {
        self.keys
            .iter()
            .map(|sk| SigningKey::new(sk.clone()))
            .collect()
    }

    /// Split into per-key [`InMemorySigner`] instances.
    ///
    /// Each signer uses a single key from the rotation pool, allowing
    /// independent send ordering per key. The global nonce manager
    /// automatically tracks nonces per `(account_id, public_key)`,
    /// so per-key signers coordinate correctly without extra setup.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::account;
    /// use near_kit_primitives::signer::{RotatingSigner, SecretKey};
    ///
    /// let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
    /// let rotating = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();
    ///
    /// // Create per-key signers for sequential queue workers
    /// let per_key: Vec<_> = rotating.into_per_key_signers();
    /// assert_eq!(per_key.len(), 2);
    /// ```
    pub fn into_per_key_signers(self) -> Vec<InMemorySigner> {
        let account_id = self.account_id;
        self.keys
            .into_iter()
            .map(|sk| InMemorySigner::new(account_id.clone(), sk))
            .collect()
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

    fn public_key(&self) -> PublicKey {
        // Peek at whichever key will be claimed next, without advancing the counter
        let idx = self.counter.load(Ordering::Relaxed) % self.keys.len();
        self.keys[idx].public_key()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;
    use crate::account;
    use crate::types::{Action, CryptoHash, NearToken, Transaction};

    #[test]
    fn test_in_memory_signer() {
        let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap());

        assert_eq!(signer.account_id().as_str(), "alice.testnet");

        let key = signer.key();
        let message = b"test message";
        let signature = block_on(key.sign(message)).unwrap();

        // Verify the key matches
        assert_eq!(key.public_key(), &signer.public_key());
        assert!(!signature.as_bytes().is_empty());
    }

    #[test]
    fn test_signature_consistency() {
        // Same message should produce the same signature (Ed25519 is deterministic)
        let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap());

        let key = signer.key();
        let message = b"test message";
        let sig1 = block_on(key.sign(message)).unwrap();
        let sig2 = block_on(key.sign(message)).unwrap();

        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_different_messages_different_signatures() {
        let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap());

        let key = signer.key();
        let sig1 = block_on(key.sign(b"message 1")).unwrap();
        let sig2 = block_on(key.sign(b"message 2")).unwrap();

        assert_ne!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_transaction_signing_with_signer_trait() {
        let secret_key = SecretKey::generate_ed25519();
        let signer = InMemorySigner::new(account!("alice.testnet"), secret_key);

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
        let signature = block_on(key.sign(tx_hash.as_bytes())).unwrap();

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

    #[test]
    fn test_rotating_signer() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();

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

    #[test]
    fn test_rotating_signer_atomic_key_claiming() {
        // Verify that key() atomically claims a key that can be used for signing
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let expected_pks: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new(account!("bot.testnet"), keys.clone()).unwrap();
        let message = b"test";

        // Claim first key and sign
        let key1 = signer.key();
        assert_eq!(key1.public_key(), &expected_pks[0]);
        let sig1 = block_on(key1.sign(message)).unwrap();
        // Verify signature matches what the raw key would produce
        let expected_sig1 = keys[0].sign(message);
        assert_eq!(sig1.as_bytes(), expected_sig1.as_bytes());

        // Claim second key and sign
        let key2 = signer.key();
        assert_eq!(key2.public_key(), &expected_pks[1]);
        let sig2 = block_on(key2.sign(message)).unwrap();
        let expected_sig2 = keys[1].sign(message);
        assert_eq!(sig2.as_bytes(), expected_sig2.as_bytes());

        // Different keys produce different signatures
        assert_ne!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_rotating_signer_empty_keys() {
        let result = RotatingSigner::new(account!("bot.testnet"), vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_signer_from_secret_key() {
        let secret = SecretKey::generate_ed25519();
        let expected_pk = secret.public_key();

        let signer = InMemorySigner::new(account!("alice.testnet"), secret);

        assert_eq!(signer.account_id().as_str(), "alice.testnet");
        assert_eq!(signer.public_key(), expected_pk);
    }

    #[test]
    fn test_implicit_account_id_is_the_hex_public_key() {
        let signer = InMemorySigner::implicit(SecretKey::generate_ed25519()).unwrap();
        let account_id = signer.account_id().as_str();

        // Implicit account IDs are 64 hex characters
        assert_eq!(account_id.len(), 64);
        assert!(account_id.chars().all(|c| c.is_ascii_hexdigit()));

        // The account ID should be the hex-encoded public key bytes
        let public_key = signer.public_key();
        let pk_bytes = public_key.as_ed25519_bytes().unwrap();
        assert_eq!(account_id, hex::encode(pk_bytes));
    }

    #[test]
    fn test_implicit() {
        let secret_key = SecretKey::generate_ed25519();
        let expected_pk = secret_key.public_key();
        let signer = InMemorySigner::implicit(secret_key).unwrap();

        // Account ID matches hex-encoded public key
        let pk_bytes = expected_pk.as_ed25519_bytes().unwrap();
        assert_eq!(signer.account_id().as_str(), hex::encode(pk_bytes));
        assert_eq!(signer.public_key(), expected_pk);
    }

    #[test]
    fn test_implicit_rejects_non_ed25519_keys() {
        for secret_key in [
            SecretKey::generate_secp256k1(),
            SecretKey::generate_ml_dsa65(),
        ] {
            assert!(matches!(
                InMemorySigner::implicit(secret_key),
                Err(SignerError::ImplicitAccountRequiresEd25519)
            ));
        }
    }

    #[test]
    fn test_implicit_accounts_of_generated_keys_differ() {
        let signer1 = InMemorySigner::implicit(SecretKey::generate_ed25519()).unwrap();
        let signer2 = InMemorySigner::implicit(SecretKey::generate_ed25519()).unwrap();
        assert_ne!(signer1.account_id(), signer2.account_id());
    }

    #[test]
    fn test_rotating_signer_holds_every_key() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];

        let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();

        assert_eq!(signer.public_keys().len(), 3);
    }

    #[test]
    fn test_in_memory_signer_debug_hides_secret() {
        let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap());

        let debug_str = format!("{:?}", signer);

        // Should show account_id and public_key but NOT the secret key
        assert!(debug_str.contains("alice.testnet"));
        assert!(debug_str.contains("public_key"));
        assert!(!debug_str.contains("secret_key"));
        assert!(!debug_str.contains("3D4YudUahN1nawWogh"));
    }

    #[test]
    fn test_rotating_signer_from_signers() {
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signers: Vec<InMemorySigner> = keys
            .into_iter()
            .map(|sk| InMemorySigner::new(account!("bot.testnet"), sk))
            .collect();

        let rotating = RotatingSigner::from_signers(signers).unwrap();
        assert_eq!(rotating.public_keys(), expected_public_keys);
    }

    #[test]
    fn test_rotating_signer_from_signers_mismatched_accounts() {
        let signers = vec![
            InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519()),
            InMemorySigner::new(account!("bob.testnet"), SecretKey::generate_ed25519()),
        ];

        let result = RotatingSigner::from_signers(signers);
        assert!(result.is_err());
    }

    #[test]
    fn test_rotating_signer_signing_keys() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();

        // signing_keys() should not advance the counter
        let counter_before = signer.counter.load(Ordering::Relaxed);
        let signing_keys = signer.signing_keys();
        let counter_after = signer.counter.load(Ordering::Relaxed);
        assert_eq!(counter_before, counter_after);

        // Should return all keys with matching public keys
        assert_eq!(signing_keys.len(), 3);
        for (sk, expected_pk) in signing_keys.iter().zip(&expected_public_keys) {
            assert_eq!(sk.public_key(), expected_pk);
        }
    }

    #[test]
    fn test_rotating_signer_into_per_key_signers() {
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let expected_public_keys: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();
        let per_key = signer.into_per_key_signers();

        assert_eq!(per_key.len(), 2);
        for (ims, expected_pk) in per_key.iter().zip(&expected_public_keys) {
            assert_eq!(ims.account_id().as_str(), "bot.testnet");
            assert_eq!(&ims.public_key(), expected_pk);
        }
    }

    #[test]
    fn test_rotating_signer_public_key_no_side_effect() {
        let keys = vec![
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
            SecretKey::generate_ed25519(),
        ];
        let pks: Vec<_> = keys.iter().map(|k| k.public_key()).collect();

        let signer = RotatingSigner::new(account!("bot.testnet"), keys).unwrap();

        // public_key() peeks at the next key without advancing the counter
        assert_eq!(signer.public_key(), pks[0]);
        assert_eq!(signer.public_key(), pks[0]);
        assert_eq!(signer.public_key(), pks[0]);

        // key() should still start from the first key (counter wasn't advanced)
        let claimed = signer.key();
        assert_eq!(claimed.public_key(), &pks[0]);

        // After one key() call, public_key() should now reflect the second key
        assert_eq!(signer.public_key(), pks[1]);

        // Claim the second key, public_key() should reflect the third
        let claimed = signer.key();
        assert_eq!(claimed.public_key(), &pks[1]);
        assert_eq!(signer.public_key(), pks[2]);

        // Claim the third key, counter wraps around to the first
        let claimed = signer.key();
        assert_eq!(claimed.public_key(), &pks[2]);
        assert_eq!(signer.public_key(), pks[0]);
    }

    #[test]
    fn test_signing_key_is_clone() {
        let signer = InMemorySigner::new(account!("alice.testnet"), "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap());

        let key = signer.key();
        let key_clone = key.clone();

        assert_eq!(key.public_key(), key_clone.public_key());
    }
}
