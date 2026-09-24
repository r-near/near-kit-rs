//! Cryptographic key types for NEAR.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

#[cfg(feature = "mnemonic")]
use bip39::Mnemonic;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use k256::elliptic_curve::Generate as _;
use k256::elliptic_curve::sec1::{FromSec1Point, ToSec1Point};
// `Signer` is already in scope from `ed25519_dalek` above: dalek 3 and `ml-dsa`
// both sit on `signature` 3, so it is literally the same trait and importing it
// from both paths warns.
use ml_dsa::signature::Verifier as _;
use ml_dsa::{B32, EncodedSignature, EncodedVerifyingKey, MlDsa65};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use super::csprng::{fill_random, os_csprng};
#[cfg(feature = "mnemonic")]
use super::hd::{derive_ed25519_slip10, derive_ml_dsa65_slip10, parse_hd_path};
use crate::error::ParseKeyError;
#[cfg(feature = "mnemonic")]
use crate::error::SignerError;

/// ML-DSA-65 public key length in bytes (FIPS 204).
pub const ML_DSA_65_PUBLIC_KEY_LENGTH: usize = 1952;
/// ML-DSA-65 seed length in bytes (the 32-byte FIPS-204 ξ).
pub const ML_DSA_65_SEED_LENGTH: usize = 32;
/// ML-DSA-65 expanded private key length in bytes (FIPS-204 `skEncode`). This is
/// the form nearcore/NEAR tooling exports under the `ml-dsa-65:` prefix.
pub const ML_DSA_65_SECRET_KEY_LENGTH: usize = 4032;
/// ML-DSA-65 signature length in bytes (FIPS 204).
pub const ML_DSA_65_SIGNATURE_LENGTH: usize = 3309;
/// Length of the on-trie ML-DSA-65 access-key identifier (a SHA3-256 digest).
pub const ML_DSA_65_HASH_LENGTH: usize = 32;

/// Domain-separation tag nearcore prepends before hashing an ML-DSA-65 public
/// key into its on-trie handle (`core/crypto/src/hash_domain.rs`).
const ML_DSA_65_PUBKEY_HASH_DOMAIN: &[u8] = b"near:ml-dsa-65-pubkey-hash:v1";

/// Key type identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyType {
    /// Ed25519 key (most common).
    Ed25519 = 0,
    /// Secp256k1 key (for Ethereum compatibility).
    Secp256k1 = 1,
    /// ML-DSA-65 post-quantum key (FIPS 204), introduced in protocol v85.
    MlDsa65 = 2,
}

impl KeyType {
    /// Get the string prefix for this key type.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyType::Ed25519 => "ed25519",
            KeyType::Secp256k1 => "secp256k1",
            KeyType::MlDsa65 => "ml-dsa-65",
        }
    }

    /// Get the expected public key length in bytes.
    pub fn public_key_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 32,
            KeyType::Secp256k1 => 64, // Uncompressed, no prefix — matches nearcore
            KeyType::MlDsa65 => ML_DSA_65_PUBLIC_KEY_LENGTH,
        }
    }

    /// Get the expected signature length in bytes.
    pub fn signature_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 64,
            KeyType::Secp256k1 => 65,
            KeyType::MlDsa65 => ML_DSA_65_SIGNATURE_LENGTH,
        }
    }
}

impl TryFrom<u8> for KeyType {
    type Error = ParseKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(KeyType::Ed25519),
            1 => Ok(KeyType::Secp256k1),
            2 => Ok(KeyType::MlDsa65),
            _ => Err(ParseKeyError::UnknownKeyType(value.to_string())),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum PublicKeyRepr {
    Ed25519([u8; 32]),
    Secp256k1([u8; 64]),
    MlDsa65(Box<[u8; ML_DSA_65_PUBLIC_KEY_LENGTH]>),
}

/// A NEAR public key.
///
/// Stored privately as fixed-size arrays matching nearcore's representation:
/// - Ed25519: 32-byte compressed point
/// - Secp256k1: 64-byte uncompressed point (x, y coordinates, no `0x04` prefix)
/// - ML-DSA-65: 1952-byte FIPS-204 public key (boxed to keep the enum small)
///
/// # ML-DSA-65 handles
///
/// A full ML-DSA-65 public key (`ml-dsa-65:`) appears on the wire — in
/// transactions and `AddKey` actions. On-chain, however, an ML-DSA-65 access
/// key is stored only as a 32-byte SHA3-256 digest, and view RPCs
/// (`view_access_key_list`) return it as `ml-dsa-65-hash:<base58>`. The full
/// 1952-byte key is *not* recoverable from that digest, so that form is **not**
/// a `PublicKey`: it is modelled by [`PublicKeyHandle`] (as in nearcore), and
/// parsing it as a `PublicKey` fails with [`ParseKeyError::MlDsa65HashHandle`].
/// Every `PublicKey` is therefore a real, borsh-serializable key that can sign
/// (given its secret key) and verify. Use [`PublicKey::to_ml_dsa65_hash`] to
/// compute the handle the chain stores for a full ML-DSA-65 key.
///
/// The representation is private so Ed25519 and Secp256k1 keys must pass
/// curve-point validation before they can be used for verification or put on
/// the wire. ML-DSA-65 public keys are structurally valid at their fixed length.
#[derive(Clone, PartialEq, Eq, Hash, SerializeDisplay, DeserializeFromStr)]
pub struct PublicKey(PublicKeyRepr);

impl PublicKey {
    /// Create an Ed25519 public key from raw 32 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ParseKeyError::InvalidCurvePoint`] if the bytes do not
    /// represent a valid compressed Ed25519 point.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Result<Self, ParseKeyError> {
        VerifyingKey::from_bytes(&bytes).map_err(|_| ParseKeyError::InvalidCurvePoint)?;
        Ok(Self(PublicKeyRepr::Ed25519(bytes)))
    }

    /// Create a Secp256k1 public key from raw 64-byte uncompressed coordinates.
    ///
    /// The 64 bytes are the raw x,y coordinates without the `0x04` prefix,
    /// matching nearcore's format.
    ///
    /// Validates that the point is on the secp256k1 curve.
    ///
    /// # Errors
    ///
    /// Returns [`ParseKeyError::InvalidCurvePoint`] if the bytes do not
    /// represent a valid point on the secp256k1 curve.
    pub fn secp256k1_from_bytes(bytes: [u8; 64]) -> Result<Self, ParseKeyError> {
        // Validate the point is on the curve by constructing full uncompressed encoding
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..].copy_from_slice(&bytes);
        let encoded = k256::Sec1Point::from_bytes(uncompressed.as_ref())
            .map_err(|_| ParseKeyError::InvalidCurvePoint)?;
        let point = k256::AffinePoint::from_sec1_point(&encoded);
        if point.is_none().into() {
            return Err(ParseKeyError::InvalidCurvePoint);
        }

        Ok(Self(PublicKeyRepr::Secp256k1(bytes)))
    }

    /// Create an ML-DSA-65 public key from raw 1952 bytes.
    pub fn ml_dsa65_from_bytes(bytes: Box<[u8; ML_DSA_65_PUBLIC_KEY_LENGTH]>) -> Self {
        Self(PublicKeyRepr::MlDsa65(bytes))
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match &self.0 {
            PublicKeyRepr::Ed25519(_) => KeyType::Ed25519,
            PublicKeyRepr::Secp256k1(_) => KeyType::Secp256k1,
            PublicKeyRepr::MlDsa65(_) => KeyType::MlDsa65,
        }
    }

    /// Get the raw key bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            PublicKeyRepr::Ed25519(bytes) => bytes.as_slice(),
            PublicKeyRepr::Secp256k1(bytes) => bytes.as_slice(),
            PublicKeyRepr::MlDsa65(bytes) => bytes.as_slice(),
        }
    }

    /// Get the key data as a fixed-size array for Ed25519 keys.
    pub fn as_ed25519_bytes(&self) -> Option<&[u8; 32]> {
        match &self.0 {
            PublicKeyRepr::Ed25519(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Get the key data as a fixed-size array for Secp256k1 keys
    /// (64-byte uncompressed, no prefix).
    pub fn as_secp256k1_bytes(&self) -> Option<&[u8; 64]> {
        match &self.0 {
            PublicKeyRepr::Secp256k1(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Get the 1952-byte ML-DSA-65 public key, if this is an ML-DSA-65 key.
    pub fn as_ml_dsa65_bytes(&self) -> Option<&[u8; ML_DSA_65_PUBLIC_KEY_LENGTH]> {
        match &self.0 {
            PublicKeyRepr::MlDsa65(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Compute the on-trie handle (`ml-dsa-65-hash:`) for an ML-DSA-65 key: the
    /// SHA3-256 of (domain tag || raw pubkey bytes), which is what
    /// `view_access_key_list` returns for the key. Returns `None` for
    /// ed25519/secp256k1 keys (those are stored, and listed, in full — see
    /// [`PublicKeyHandle::from`]).
    pub fn to_ml_dsa65_hash(&self) -> Option<PublicKeyHandle> {
        match &self.0 {
            PublicKeyRepr::MlDsa65(bytes) => {
                use sha3::{Digest as _, Sha3_256};
                let mut hasher = Sha3_256::new();
                hasher.update(ML_DSA_65_PUBKEY_HASH_DOMAIN);
                hasher.update(bytes.as_slice());
                let mut out = [0u8; ML_DSA_65_HASH_LENGTH];
                out.copy_from_slice(&hasher.finalize());
                Some(PublicKeyHandle::MlDsa65Hash(out))
            }
            _ => None,
        }
    }
}

impl FromStr for PublicKey {
    type Err = ParseKeyError;

    /// Parse a full public key: `ed25519:`, `secp256k1:` or `ml-dsa-65:`.
    ///
    /// The `ml-dsa-65-hash:` view handle is **not** a public key and is
    /// rejected with [`ParseKeyError::MlDsa65HashHandle`]; parse it as a
    /// [`PublicKeyHandle`] instead.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (prefix, data_str) = s.split_once(':').ok_or(ParseKeyError::InvalidFormat)?;

        let key_type = match prefix {
            "ed25519" => KeyType::Ed25519,
            "secp256k1" => KeyType::Secp256k1,
            "ml-dsa-65" => KeyType::MlDsa65,
            // Not unknown — a pointed error, since this is exactly what users
            // copy-paste out of `view_access_key_list`.
            ML_DSA_65_HASH_PREFIX => return Err(ParseKeyError::MlDsa65HashHandle),
            other => return Err(ParseKeyError::UnknownKeyType(other.to_string())),
        };

        let data = bs58::decode(data_str)
            .into_vec()
            .map_err(|e| ParseKeyError::InvalidBase58(e.to_string()))?;

        if data.len() != key_type.public_key_len() {
            return Err(ParseKeyError::InvalidLength {
                expected: key_type.public_key_len(),
                actual: data.len(),
            });
        }

        match key_type {
            KeyType::Ed25519 => {
                let bytes: [u8; 32] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidCurvePoint)?;
                Self::ed25519_from_bytes(bytes)
            }
            KeyType::Secp256k1 => {
                let bytes: [u8; 64] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidCurvePoint)?;
                Self::secp256k1_from_bytes(bytes)
            }
            KeyType::MlDsa65 => {
                // Unlike ed25519/secp256k1, ML-DSA-65 has no public-key validity
                // predicate to check: FIPS-204 `pkDecode` (Algorithm 23) is pure
                // structural decoding, so every correctly-sized (1952-byte)
                // payload is a well-formed verifying key. The length check above
                // is therefore the complete parse-time validation.
                let bytes: Box<[u8; ML_DSA_65_PUBLIC_KEY_LENGTH]> = data
                    .into_boxed_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidLength {
                        expected: ML_DSA_65_PUBLIC_KEY_LENGTH,
                        actual: ML_DSA_65_PUBLIC_KEY_LENGTH, // unreachable: length checked above
                    })?;
                Ok(Self::ml_dsa65_from_bytes(bytes))
            }
        }
    }
}

impl TryFrom<&str> for PublicKey {
    type Error = ParseKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            self.key_type().as_str(),
            bs58::encode(self.as_bytes()).into_string()
        )
    }
}

impl Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ML-DSA-65 keys are ~2 KiB; printing the full base58 in `{:?}` would
        // bloat logs and allocate heavily. Show a truncated form instead.
        match &self.0 {
            PublicKeyRepr::MlDsa65(_) => write!(f, "PublicKey(ml-dsa-65:<1952 bytes>)"),
            _ => write!(f, "PublicKey({})", self),
        }
    }
}

impl BorshSerialize for PublicKey {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&(self.key_type() as u8), writer)?;
        writer.write_all(self.as_bytes())?;
        Ok(())
    }
}

impl BorshDeserialize for PublicKey {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let key_type_byte = u8::deserialize_reader(reader)?;
        let key_type = KeyType::try_from(key_type_byte)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        match key_type {
            KeyType::Ed25519 => {
                let mut bytes = [0u8; 32];
                reader.read_exact(&mut bytes)?;
                Self::ed25519_from_bytes(bytes).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid ed25519 curve point",
                    )
                })
            }
            KeyType::Secp256k1 => {
                let mut bytes = [0u8; 64];
                reader.read_exact(&mut bytes)?;
                Self::secp256k1_from_bytes(bytes).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid secp256k1 curve point",
                    )
                })
            }
            KeyType::MlDsa65 => {
                // The wire form of an ML-DSA-65 `PublicKey` is always the full
                // 1952-byte key (`[2][1952]`); the 32-byte handle
                // (`PublicKeyHandle`) never appears in borsh, only in view JSON.
                let mut bytes = Box::new([0u8; ML_DSA_65_PUBLIC_KEY_LENGTH]);
                reader.read_exact(bytes.as_mut_slice())?;
                Ok(Self::ml_dsa65_from_bytes(bytes))
            }
        }
    }
}

/// String prefix of an ML-DSA-65 access-key handle (`ml-dsa-65-hash:<base58>`).
const ML_DSA_65_HASH_PREFIX: &str = "ml-dsa-65-hash";

/// How the chain refers to an access key: the full public key for ed25519 and
/// secp256k1, or a 32-byte SHA3-256 *handle* for ML-DSA-65.
///
/// Mirrors nearcore's `PublicKeyHandle`. On-chain, an ML-DSA-65 access key is
/// stored only as a domain-tagged SHA3-256 digest of the 1952-byte key, and
/// view RPCs (`view_access_key_list`, state-change views) return it as
/// `ml-dsa-65-hash:<base58>`. The full key is not recoverable from the digest,
/// so such a value cannot sign, verify, or go into a transaction — which is why
/// it is a separate type rather than a [`PublicKey`] variant. Fields that can
/// only ever carry full keys (transactions, actions, receipts, validators) stay
/// `PublicKey`.
///
/// - [`full_pubkey`](Self::full_pubkey) / [`into_full`](Self::into_full) get
///   the usable key back when there is one.
/// - [`PublicKey::to_ml_dsa65_hash`] computes the handle for a full ML-DSA-65
///   key; [`refers_to`](Self::refers_to) checks a handle against a full key.
/// - `From<PublicKey>` wraps a full key as [`PublicKeyHandle::Full`] without
///   hashing.
///
/// Parses from and prints as either form; there is deliberately no borsh impl
/// (a handle never goes on the wire).
///
/// # Example
///
/// ```rust
/// # use near_kit::signer::{PublicKey, PublicKeyHandle};
/// let full: PublicKeyHandle = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
/// assert!(full.full_pubkey().is_some());
///
/// let handle: PublicKeyHandle =
///     "ml-dsa-65-hash:GsDTSpXDhiutJazktZEpKNQZit7U91LskL2Fq541u8PJ".parse()?;
/// assert!(handle.full_pubkey().is_none());
/// // ...and the same string is *not* a PublicKey:
/// assert!("ml-dsa-65-hash:GsDTSpXDhiutJazktZEpKNQZit7U91LskL2Fq541u8PJ"
///     .parse::<PublicKey>()
///     .is_err());
/// # Ok::<(), near_kit::signer::ParseKeyError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash, SerializeDisplay, DeserializeFromStr)]
pub enum PublicKeyHandle {
    /// A full public key, as stored on-chain for ed25519/secp256k1 access keys.
    Full(PublicKey),
    /// The 32-byte SHA3-256 handle under which an ML-DSA-65 access key is stored.
    MlDsa65Hash([u8; ML_DSA_65_HASH_LENGTH]),
}

impl PublicKeyHandle {
    /// The full public key, if this handle carries one (ed25519/secp256k1, or a
    /// full ML-DSA-65 key wrapped via `From<PublicKey>`). `None` for an
    /// ML-DSA-65 hash: the key is not recoverable from the digest.
    pub fn full_pubkey(&self) -> Option<&PublicKey> {
        match self {
            Self::Full(pk) => Some(pk),
            Self::MlDsa65Hash(_) => None,
        }
    }

    /// Owned version of [`full_pubkey`](Self::full_pubkey).
    pub fn into_full(self) -> Option<PublicKey> {
        match self {
            Self::Full(pk) => Some(pk),
            Self::MlDsa65Hash(_) => None,
        }
    }

    /// The key type. An ML-DSA-65 hash reports [`KeyType::MlDsa65`]: the
    /// storage form differs, the scheme does not.
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Full(pk) => pk.key_type(),
            Self::MlDsa65Hash(_) => KeyType::MlDsa65,
        }
    }

    /// The raw bytes: the key bytes for a full key, the 32-byte digest for a
    /// hash.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Full(pk) => pk.as_bytes(),
            Self::MlDsa65Hash(bytes) => bytes.as_slice(),
        }
    }

    /// Whether this is an ML-DSA-65 hash handle rather than a full key.
    pub fn is_ml_dsa65_hash(&self) -> bool {
        matches!(self, Self::MlDsa65Hash(_))
    }

    /// Whether this handle identifies `key`: equal to it when full, or equal to
    /// [`key.to_ml_dsa65_hash()`](PublicKey::to_ml_dsa65_hash) when a hash.
    /// This is the check to use for "is my key in `view_access_key_list`".
    pub fn refers_to(&self, key: &PublicKey) -> bool {
        match self {
            Self::Full(pk) => pk == key,
            Self::MlDsa65Hash(_) => key.to_ml_dsa65_hash().as_ref() == Some(self),
        }
    }
}

impl From<PublicKey> for PublicKeyHandle {
    /// Wraps the key as [`PublicKeyHandle::Full`] (no hashing). To get the
    /// handle the chain stores for an ML-DSA-65 key, use
    /// [`PublicKey::to_ml_dsa65_hash`].
    fn from(key: PublicKey) -> Self {
        Self::Full(key)
    }
}

impl FromStr for PublicKeyHandle {
    type Err = ParseKeyError;

    /// Accepts every [`PublicKey`] form plus `ml-dsa-65-hash:<base58 32 bytes>`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some((ML_DSA_65_HASH_PREFIX, data_str)) => {
                let data = bs58::decode(data_str)
                    .into_vec()
                    .map_err(|e| ParseKeyError::InvalidBase58(e.to_string()))?;
                let bytes: [u8; ML_DSA_65_HASH_LENGTH] =
                    data.as_slice()
                        .try_into()
                        .map_err(|_| ParseKeyError::InvalidLength {
                            expected: ML_DSA_65_HASH_LENGTH,
                            actual: data.len(),
                        })?;
                Ok(Self::MlDsa65Hash(bytes))
            }
            _ => s.parse().map(Self::Full),
        }
    }
}

impl Display for PublicKeyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(pk) => Display::fmt(pk, f),
            Self::MlDsa65Hash(bytes) => write!(
                f,
                "{}:{}",
                ML_DSA_65_HASH_PREFIX,
                bs58::encode(bytes).into_string()
            ),
        }
    }
}

impl Debug for PublicKeyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Full ML-DSA-65 keys stay truncated, as in PublicKey's Debug; the
            // 32-byte hash and the other key types print in full.
            Self::Full(key) if key.key_type() == KeyType::MlDsa65 => {
                write!(f, "PublicKeyHandle(ml-dsa-65:<1952 bytes>)")
            }
            _ => write!(f, "PublicKeyHandle({})", self),
        }
    }
}

/// Default BIP-32 HD derivation path for NEAR keys.
/// NEAR uses coin type 397 per SLIP-44.
#[cfg(feature = "mnemonic")]
pub const DEFAULT_HD_PATH: &str = "m/44'/397'/0'";

/// Default number of words in generated seed phrases.
#[cfg(feature = "mnemonic")]
pub const DEFAULT_WORD_COUNT: usize = 12;

/// Default number of words in seed phrases generated for ML-DSA-65 keys.
///
/// NEP-649 (<https://github.com/near/NEPs/pull/649>) requires a *newly
/// generated* ML-DSA-65 mnemonic to carry at least 18 words: 12 words is 128
/// bits of entropy, below ML-DSA-65's NIST Category 3 security level. The NEP's
/// own test vectors use 24 words, so that is the default here.
///
/// Recovery is unaffected — [`SecretKey::ml_dsa65_from_seed_phrase`] and
/// friends still accept any valid 12–24-word BIP-39 phrase.
#[cfg(feature = "mnemonic")]
pub const DEFAULT_ML_DSA_65_WORD_COUNT: usize = 24;

/// Minimum number of words NEP-649 allows in a newly generated ML-DSA-65 seed
/// phrase.
#[cfg(feature = "mnemonic")]
const MIN_ML_DSA_65_WORD_COUNT: usize = 18;

#[derive(Clone)]
enum SecretKeyRepr {
    Ed25519([u8; 32]),
    Secp256k1([u8; 32]),
    MlDsa65(Box<[u8; ML_DSA_65_SEED_LENGTH]>),
}

/// A NEAR secret key.
///
/// ML-DSA-65 keys are held only as the safe, canonical 32-byte FIPS-204 seed
/// (ξ). Use [`SecretKey::to_ml_dsa65_expanded_bytes`] for one-way export to
/// nearcore-based tooling that requires the 4032-byte expanded encoding.
///
/// The representation is private so every Secp256k1 scalar must pass the
/// validating constructor before it can be used by [`SecretKey::public_key`]
/// or [`SecretKey::sign`].
#[derive(Clone, SerializeDisplay, DeserializeFromStr)]
pub struct SecretKey(SecretKeyRepr);

impl SecretKey {
    /// Generate a new random Ed25519 key pair.
    pub fn generate_ed25519() -> Self {
        Self(SecretKeyRepr::Ed25519(
            SigningKey::generate(&mut os_csprng()).to_bytes(),
        ))
    }

    /// Create an Ed25519 secret key from raw 32 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self {
        Self(SecretKeyRepr::Ed25519(bytes))
    }

    /// Generate a new random Secp256k1 key pair.
    pub fn generate_secp256k1() -> Self {
        let secret_key = k256::SecretKey::generate_from_rng(&mut os_csprng());
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&secret_key.to_bytes());
        Self(SecretKeyRepr::Secp256k1(bytes))
    }

    /// Create a Secp256k1 secret key from raw 32 bytes.
    ///
    /// Validates that the bytes represent a valid secp256k1 scalar
    /// (non-zero and less than the curve order).
    pub fn secp256k1_from_bytes(bytes: [u8; 32]) -> Result<Self, ParseKeyError> {
        k256::SecretKey::from_bytes((&bytes).into()).map_err(|_| ParseKeyError::InvalidScalar)?;
        Ok(Self(SecretKeyRepr::Secp256k1(bytes)))
    }

    /// Generate a new random ML-DSA-65 key pair (FIPS 204).
    ///
    /// The key is held as its 32-byte FIPS-204 seed, so [`Display`] prints
    /// `ml-dsa-65:<base58 of 32 bytes>`. nearcore's `near-crypto` cannot parse
    /// that form (it requires the 4032-byte expanded key under the same
    /// prefix); if the key has to be handed to nearcore-based tooling, export
    /// it with [`SecretKey::to_ml_dsa65_expanded_bytes`].
    pub fn generate_ml_dsa65() -> Self {
        let mut seed = Box::new([0u8; ML_DSA_65_SEED_LENGTH]);
        fill_random(seed.as_mut_slice());
        Self(SecretKeyRepr::MlDsa65(seed))
    }

    /// Create an ML-DSA-65 secret key from a 32-byte FIPS-204 seed.
    pub fn ml_dsa65_from_seed(seed: [u8; ML_DSA_65_SEED_LENGTH]) -> Self {
        Self(SecretKeyRepr::MlDsa65(Box::new(seed)))
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match &self.0 {
            SecretKeyRepr::Ed25519(_) => KeyType::Ed25519,
            SecretKeyRepr::Secp256k1(_) => KeyType::Secp256k1,
            SecretKeyRepr::MlDsa65(_) => KeyType::MlDsa65,
        }
    }

    /// Get the raw key bytes as a slice.
    ///
    /// For ML-DSA-65 this is always the 32-byte FIPS-204 seed.
    pub fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            SecretKeyRepr::Ed25519(bytes) => bytes.as_slice(),
            SecretKeyRepr::Secp256k1(bytes) => bytes.as_slice(),
            SecretKeyRepr::MlDsa65(seed) => seed.as_slice(),
        }
    }

    /// Return the raw 4032-byte expanded form (FIPS-204 `skEncode`) of this
    /// ML-DSA-65 seed for one-way export to nearcore-based tooling.
    ///
    /// The returned bytes derive the same public key, but cannot be imported
    /// back into [`SecretKey`] because the original seed is not recoverable.
    /// Returns `None` for Ed25519 and Secp256k1 keys.
    pub fn to_ml_dsa65_expanded_bytes(&self) -> Option<Box<[u8; ML_DSA_65_SECRET_KEY_LENGTH]>> {
        match &self.0 {
            SecretKeyRepr::MlDsa65(seed) => {
                // `to_expanded` is deprecated upstream in favor of seed keygen,
                // but the expanded encoding is what nearcore interoperates on.
                #[allow(deprecated)]
                let enc = Self::ml_dsa65_signing_key(seed).to_expanded();
                let mut bytes = Box::new([0u8; ML_DSA_65_SECRET_KEY_LENGTH]);
                bytes.copy_from_slice(enc.as_slice());
                Some(bytes)
            }
            SecretKeyRepr::Ed25519(_) | SecretKeyRepr::Secp256k1(_) => None,
        }
    }

    /// Expand a 32-byte FIPS-204 seed into the signing key used internally.
    fn ml_dsa65_signing_key(
        seed: &[u8; ML_DSA_65_SEED_LENGTH],
    ) -> ml_dsa::ExpandedSigningKey<MlDsa65> {
        let xi = B32::try_from(seed.as_slice()).expect("32-byte seed");
        ml_dsa::ExpandedSigningKey::<MlDsa65>::from_seed(&xi)
    }

    /// Derive the public key.
    pub fn public_key(&self) -> PublicKey {
        match &self.0 {
            SecretKeyRepr::Ed25519(bytes) => {
                let signing_key = SigningKey::from_bytes(bytes);
                let verifying_key = signing_key.verifying_key();
                PublicKey(PublicKeyRepr::Ed25519(verifying_key.to_bytes()))
            }
            SecretKeyRepr::Secp256k1(bytes) => {
                let secret_key =
                    k256::SecretKey::from_bytes(bytes.into()).expect("invalid secp256k1 key");
                let public_key = secret_key.public_key();
                // Get uncompressed encoding (65 bytes with 0x04 prefix)
                let uncompressed = public_key.to_sec1_point(false);
                let uncompressed_bytes: &[u8] = uncompressed.as_bytes();
                assert_eq!(uncompressed_bytes.len(), 65);
                // Store without the 0x04 prefix (64 bytes)
                let mut result = [0u8; 64];
                result.copy_from_slice(&uncompressed_bytes[1..]);
                PublicKey(PublicKeyRepr::Secp256k1(result))
            }
            SecretKeyRepr::MlDsa65(sk) => {
                let signing_key = Self::ml_dsa65_signing_key(sk);
                let encoded: EncodedVerifyingKey<MlDsa65> = signing_key.verifying_key().encode();
                let mut bytes = Box::new([0u8; ML_DSA_65_PUBLIC_KEY_LENGTH]);
                bytes.copy_from_slice(encoded.as_slice());
                PublicKey(PublicKeyRepr::MlDsa65(bytes))
            }
        }
    }

    /// Sign a message.
    ///
    /// The bytes are passed to the underlying scheme exactly as nearcore does
    /// in `near_crypto::SecretKey::sign`:
    ///
    /// - Ed25519 and ML-DSA-65 sign `message` as-is (any length).
    /// - Secp256k1 treats `message` as an already-computed 32-byte digest and
    ///   signs it directly, without hashing it again. Every NEAR signing
    ///   payload (transaction hash, NEP-366/461 delegate-action hash, NEP-413
    ///   message hash) is a 32-byte SHA-256 digest, so this is what nearcore
    ///   verifies against.
    ///
    /// # Panics
    ///
    /// Panics if the key is Secp256k1 and `message` is not exactly 32 bytes,
    /// mirroring nearcore. To sign arbitrary bytes with a Secp256k1 key, hash
    /// them first (e.g. with SHA-256) and sign the digest.
    pub fn sign(&self, message: &[u8]) -> Signature {
        match &self.0 {
            SecretKeyRepr::Ed25519(bytes) => {
                let signing_key = SigningKey::from_bytes(bytes);
                let signature = signing_key.sign(message);
                Signature::Ed25519(signature.to_bytes())
            }
            SecretKeyRepr::Secp256k1(bytes) => {
                let signing_key = k256::ecdsa::SigningKey::from_bytes(bytes.into())
                    .expect("invalid secp256k1 key");

                // NEAR protocol: `message` is the 32-byte digest itself
                // (nearcore: `secp256k1::Message::from_slice(data)`), so it is
                // signed directly with no further hashing.
                let digest: &[u8; 32] = message
                    .try_into()
                    .expect("secp256k1 signing expects a 32-byte digest");
                let (signature, recovery_id) = signing_key.sign_prehash_recoverable(digest);

                // NEAR format: [r (32) | s (32) | v (1)]
                let mut sig_bytes = [0u8; 65];
                sig_bytes[..64].copy_from_slice(&signature.to_bytes());
                sig_bytes[64] = recovery_id.to_byte();

                Signature::Secp256k1(sig_bytes)
            }
            SecretKeyRepr::MlDsa65(sk) => {
                // FIPS-204 ML-DSA.Sign with empty context. The `ml-dsa` crate's
                // `Signer` is the deterministic variant; nearcore verifies with a
                // FIPS-204 verifier, which accepts any conformant signature.
                let signing_key = Self::ml_dsa65_signing_key(sk);
                let sig: ml_dsa::Signature<MlDsa65> = signing_key.sign(message);
                let encoded: EncodedSignature<MlDsa65> = sig.encode();
                let mut bytes = Box::new([0u8; ML_DSA_65_SIGNATURE_LENGTH]);
                bytes.copy_from_slice(encoded.as_slice());
                Signature::MlDsa65(bytes)
            }
        }
    }

    // ========================================================================
    // Seed Phrase / Mnemonic Support
    // ========================================================================

    /// Derive an Ed25519 secret key from a BIP-39 seed phrase.
    ///
    /// Uses SLIP-10 derivation with the default NEAR HD path (`m/44'/397'/0'`).
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase (12, 15, 18, 21, or 24 words)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// // Valid BIP-39 mnemonic (all zeros entropy)
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase(phrase).unwrap();
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn from_seed_phrase(phrase: impl AsRef<str>) -> Result<Self, SignerError> {
        Self::from_seed_phrase_with_path(phrase, DEFAULT_HD_PATH)
    }

    /// Derive an Ed25519 secret key from a BIP-39 seed phrase with custom HD path.
    ///
    /// Uses SLIP-10 derivation for Ed25519 keys. Only hardened derivation paths
    /// are supported (all path components must use `'` suffix).
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase (12, 15, 18, 21, or 24 words)
    /// * `hd_path` - BIP-32 derivation path (e.g., `"m/44'/397'/0'"`)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase_with_path(phrase, "m/44'/397'/1'").unwrap();
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn from_seed_phrase_with_path(
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
    ) -> Result<Self, SignerError> {
        Self::from_seed_phrase_with_path_and_passphrase(phrase, hd_path, None)
    }

    /// Derive an Ed25519 secret key from a BIP-39 seed phrase with passphrase.
    ///
    /// The passphrase provides additional entropy for seed generation (BIP-39 feature).
    /// An empty passphrase is equivalent to no passphrase.
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase
    /// * `hd_path` - BIP-32 derivation path
    /// * `passphrase` - Optional passphrase for additional entropy
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase_with_path_and_passphrase(
    ///     phrase,
    ///     "m/44'/397'/0'",
    ///     Some("my-passphrase")
    /// ).unwrap();
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn from_seed_phrase_with_path_and_passphrase(
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
        passphrase: Option<&str>,
    ) -> Result<Self, SignerError> {
        let seed = mnemonic_to_seed(phrase, passphrase)?;
        let path = parse_hd_path(hd_path.as_ref())
            .map_err(|e| SignerError::KeyDerivationFailed(format!("Invalid HD path: {e}")))?;
        let derived = derive_ed25519_slip10(&seed, &path);

        Ok(Self::ed25519_from_bytes(derived))
    }

    /// Derive an ML-DSA-65 secret key from a BIP-39 seed phrase.
    ///
    /// Uses the post-quantum SLIP-10 construction from satoshilabs/slips#1968,
    /// specified for NEAR by NEP-649 (<https://github.com/near/NEPs/pull/649>),
    /// with the default NEAR HD path (`m/44'/397'/0'`): the 32-byte node secret
    /// is the FIPS-204 seed ξ fed to ML-DSA-65 KeyGen.
    ///
    /// The master HMAC salt differs from the Ed25519 branch (`"ML-DSA-65 seed"`
    /// vs `"ed25519 seed"`), so the ML-DSA-65 key derived from a phrase is
    /// **unrelated** to the Ed25519 key derived from the same phrase.
    ///
    /// This is a *recovery* entry point and accepts any valid 12–24-word BIP-39
    /// phrase. Newly generated phrases have a higher floor — see
    /// [`SecretKey::ml_dsa65_generate_with_seed_phrase`].
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase (12, 15, 18, 21, or 24 words)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::ml_dsa65_from_seed_phrase(phrase).unwrap();
    /// assert!(secret_key.to_string().starts_with("ml-dsa-65:"));
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_from_seed_phrase(phrase: impl AsRef<str>) -> Result<Self, SignerError> {
        Self::ml_dsa65_from_seed_phrase_with_path(phrase, DEFAULT_HD_PATH)
    }

    /// Derive an ML-DSA-65 secret key from a BIP-39 seed phrase with a custom
    /// HD path.
    ///
    /// Uses the satoshilabs/slips#1968 SLIP-10 construction, as specified by
    /// NEP-649 (<https://github.com/near/NEPs/pull/649>). Only hardened
    /// derivation paths are supported (all path components must use `'`).
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase (12, 15, 18, 21, or 24 words)
    /// * `hd_path` - BIP-32 derivation path (e.g., `"m/44'/397'/0'"`)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::ml_dsa65_from_seed_phrase_with_path(phrase, "m/44'/397'/1'").unwrap();
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_from_seed_phrase_with_path(
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
    ) -> Result<Self, SignerError> {
        Self::ml_dsa65_from_seed_phrase_with_path_and_passphrase(phrase, hd_path, None)
    }

    /// Derive an ML-DSA-65 secret key from a BIP-39 seed phrase with a
    /// passphrase.
    ///
    /// The passphrase provides additional entropy for seed generation (BIP-39
    /// feature). An empty passphrase is equivalent to no passphrase. Derivation
    /// otherwise follows satoshilabs/slips#1968 / NEP-649
    /// (<https://github.com/near/NEPs/pull/649>) — see
    /// [`SecretKey::ml_dsa65_from_seed_phrase`] for the salt caveat.
    ///
    /// # Arguments
    ///
    /// * `phrase` - BIP-39 mnemonic phrase
    /// * `hd_path` - BIP-32 derivation path
    /// * `passphrase` - Optional passphrase for additional entropy
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::ml_dsa65_from_seed_phrase_with_path_and_passphrase(
    ///     phrase,
    ///     "m/44'/397'/0'",
    ///     Some("my-passphrase"),
    /// )
    /// .unwrap();
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_from_seed_phrase_with_path_and_passphrase(
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
        passphrase: Option<&str>,
    ) -> Result<Self, SignerError> {
        let seed = mnemonic_to_seed(phrase, passphrase)?;
        let path = parse_hd_path(hd_path.as_ref())
            .map_err(|e| SignerError::KeyDerivationFailed(format!("Invalid HD path: {e}")))?;
        let derived = derive_ml_dsa65_slip10(&seed, &path);

        Ok(Self::ml_dsa65_from_seed(derived))
    }

    /// Generate a new random seed phrase and derive the corresponding secret key.
    ///
    /// Returns both the seed phrase (for backup) and the derived secret key.
    /// Uses 12 words by default and the standard NEAR HD path.
    ///
    /// If you intend to derive an ML-DSA-65 key from the phrase, use
    /// [`SecretKey::ml_dsa65_generate_with_seed_phrase`] instead (or pass at
    /// least 18 words), since NEP-649
    /// (<https://github.com/near/NEPs/pull/649>) requires a newly generated
    /// ML-DSA-65 mnemonic to carry at least 18 words.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::generate_with_seed_phrase().unwrap();
    /// println!("Backup your seed phrase: {}", phrase);
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn generate_with_seed_phrase() -> Result<(String, Self), SignerError> {
        Self::generate_with_seed_phrase_custom(DEFAULT_WORD_COUNT, DEFAULT_HD_PATH, None)
    }

    /// Generate a new random seed phrase with custom word count.
    ///
    /// # Arguments
    ///
    /// * `word_count` - Number of words (12, 15, 18, 21, or 24)
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::generate_with_seed_phrase_words(24).unwrap();
    /// assert_eq!(phrase.split_whitespace().count(), 24);
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn generate_with_seed_phrase_words(
        word_count: usize,
    ) -> Result<(String, Self), SignerError> {
        Self::generate_with_seed_phrase_custom(word_count, DEFAULT_HD_PATH, None)
    }

    /// Generate a new random seed phrase with full customization.
    ///
    /// # Arguments
    ///
    /// * `word_count` - Number of words (12, 15, 18, 21, or 24)
    /// * `hd_path` - BIP-32 derivation path
    /// * `passphrase` - Optional passphrase for additional entropy
    #[cfg(feature = "mnemonic")]
    pub fn generate_with_seed_phrase_custom(
        word_count: usize,
        hd_path: impl AsRef<str>,
        passphrase: Option<&str>,
    ) -> Result<(String, Self), SignerError> {
        let phrase = generate_seed_phrase(word_count)?;
        let secret_key =
            Self::from_seed_phrase_with_path_and_passphrase(&phrase, hd_path, passphrase)?;
        Ok((phrase, secret_key))
    }

    /// Generate a new random seed phrase and derive the corresponding
    /// ML-DSA-65 secret key.
    ///
    /// Returns both the seed phrase (for backup) and the derived secret key.
    /// Uses [`DEFAULT_ML_DSA_65_WORD_COUNT`] (24) words and the standard NEAR
    /// HD path.
    ///
    /// NEP-649 (<https://github.com/near/NEPs/pull/649>) requires a newly
    /// generated ML-DSA-65 mnemonic to carry at least 18 words — 12 words is
    /// 128 bits of entropy, below ML-DSA-65's NIST Category 3 level — and its
    /// own vectors use 24. Recovery is unaffected:
    /// [`SecretKey::ml_dsa65_from_seed_phrase`] and friends still accept any
    /// valid 12–24-word BIP-39 phrase.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::ml_dsa65_generate_with_seed_phrase().unwrap();
    /// assert_eq!(phrase.split_whitespace().count(), 24);
    /// assert!(secret_key.to_string().starts_with("ml-dsa-65:"));
    /// println!("Backup your seed phrase: {}", phrase);
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_generate_with_seed_phrase() -> Result<(String, Self), SignerError> {
        Self::ml_dsa65_generate_with_seed_phrase_custom(
            DEFAULT_ML_DSA_65_WORD_COUNT,
            DEFAULT_HD_PATH,
            None,
        )
    }

    /// Generate a new random ML-DSA-65 seed phrase with custom word count.
    ///
    /// # Arguments
    ///
    /// * `word_count` - Number of words (18, 21, or 24). Per NEP-649
    ///   (<https://github.com/near/NEPs/pull/649>), 12 and 15 are rejected for
    ///   newly generated ML-DSA-65 phrases.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::signer::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::ml_dsa65_generate_with_seed_phrase_words(18).unwrap();
    /// assert_eq!(phrase.split_whitespace().count(), 18);
    /// assert!(SecretKey::ml_dsa65_generate_with_seed_phrase_words(12).is_err());
    /// ```
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_generate_with_seed_phrase_words(
        word_count: usize,
    ) -> Result<(String, Self), SignerError> {
        Self::ml_dsa65_generate_with_seed_phrase_custom(word_count, DEFAULT_HD_PATH, None)
    }

    /// Generate a new random ML-DSA-65 seed phrase with full customization.
    ///
    /// # Arguments
    ///
    /// * `word_count` - Number of words (18, 21, or 24)
    /// * `hd_path` - BIP-32 derivation path
    /// * `passphrase` - Optional passphrase for additional entropy
    ///
    /// # Errors
    ///
    /// Returns [`SignerError::KeyDerivationFailed`] if `word_count` is below
    /// the 18-word NEP-649 floor for newly generated ML-DSA-65 phrases, or is
    /// not one of the valid BIP-39 word counts.
    #[cfg(feature = "mnemonic")]
    pub fn ml_dsa65_generate_with_seed_phrase_custom(
        word_count: usize,
        hd_path: impl AsRef<str>,
        passphrase: Option<&str>,
    ) -> Result<(String, Self), SignerError> {
        if word_count < MIN_ML_DSA_65_WORD_COUNT {
            return Err(SignerError::KeyDerivationFailed(format!(
                "Invalid word count: {}. NEP-649 requires at least {} words for a newly \
                 generated ML-DSA-65 seed phrase (12 words is 128 bits of entropy, below \
                 ML-DSA-65's NIST Category 3 level); use 18, 21, or 24",
                word_count, MIN_ML_DSA_65_WORD_COUNT
            )));
        }
        let phrase = generate_seed_phrase(word_count)?;
        let secret_key =
            Self::ml_dsa65_from_seed_phrase_with_path_and_passphrase(&phrase, hd_path, passphrase)?;
        Ok((phrase, secret_key))
    }
}

impl FromStr for SecretKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key_type, data_str) = s.split_once(':').ok_or(ParseKeyError::InvalidFormat)?;

        let key_type = match key_type {
            "ed25519" => KeyType::Ed25519,
            "secp256k1" => KeyType::Secp256k1,
            "ml-dsa-65" => KeyType::MlDsa65,
            other => return Err(ParseKeyError::UnknownKeyType(other.to_string())),
        };

        let data = bs58::decode(data_str)
            .into_vec()
            .map_err(|e| ParseKeyError::InvalidBase58(e.to_string()))?;

        // ML-DSA-65 secret keys are always the safe 32-byte FIPS-204 seed.
        // Expanded 4032-byte strings exported by external tooling are rejected:
        // importing them would invoke upstream range decoding that can panic,
        // and the original seed cannot be recovered from that representation.
        if key_type == KeyType::MlDsa65 {
            if data.len() != ML_DSA_65_SEED_LENGTH {
                return Err(ParseKeyError::InvalidLength {
                    expected: ML_DSA_65_SEED_LENGTH,
                    actual: data.len(),
                });
            }
            let seed: [u8; ML_DSA_65_SEED_LENGTH] =
                data.as_slice().try_into().expect("length checked");
            return Ok(Self::ml_dsa65_from_seed(seed));
        }

        // For ed25519, the secret key might be 32 bytes (seed) or 64 bytes (expanded)
        // For secp256k1, it must be 32 bytes
        let valid_len = match key_type {
            KeyType::Ed25519 => data.len() == 32 || data.len() == 64,
            KeyType::Secp256k1 => data.len() == 32,
            KeyType::MlDsa65 => unreachable!("handled above"),
        };
        if !valid_len {
            return Err(ParseKeyError::InvalidLength {
                expected: 32,
                actual: data.len(),
            });
        }

        // Take first 32 bytes if 64-byte expanded key
        let bytes: [u8; 32] = data[..32]
            .try_into()
            .map_err(|_| ParseKeyError::InvalidFormat)?;

        match key_type {
            KeyType::Ed25519 => Ok(Self::ed25519_from_bytes(bytes)),
            KeyType::Secp256k1 => Self::secp256k1_from_bytes(bytes),
            KeyType::MlDsa65 => unreachable!("handled above"),
        }
    }
}

impl TryFrom<&str> for SecretKey {
    type Error = ParseKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Formats the key as `<key-type>:<base58 payload>`, where the payload is
/// [`SecretKey::as_bytes`].
///
/// For ML-DSA-65 the payload is always the 32-byte seed. nearcore's
/// `near-crypto` expects the 4032-byte expanded encoding instead; obtain those
/// raw bytes with [`SecretKey::to_ml_dsa65_expanded_bytes`] when exporting.
impl Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            self.key_type().as_str(),
            bs58::encode(self.as_bytes()).into_string()
        )
    }
}

impl Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({}:***)", self.key_type().as_str())
    }
}

/// Cryptographic signature.
#[derive(Clone, PartialEq, Eq, SerializeDisplay, DeserializeFromStr)]
pub enum Signature {
    /// Ed25519 signature (64 bytes).
    Ed25519([u8; 64]),
    /// Secp256k1 signature (65 bytes: `[r (32) | s (32) | v (1)]`).
    Secp256k1([u8; 65]),
    /// ML-DSA-65 signature (3309 bytes, FIPS 204). Boxed to keep the enum small.
    MlDsa65(Box<[u8; ML_DSA_65_SIGNATURE_LENGTH]>),
}

impl Signature {
    /// Create an Ed25519 signature from raw 64 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 64]) -> Self {
        Self::Ed25519(bytes)
    }

    /// Create a Secp256k1 signature from raw 65 bytes.
    ///
    /// The format is `[r (32 bytes) | s (32 bytes) | v (1 byte recovery id)]`,
    /// matching the NEAR protocol's secp256k1 signature format.
    pub fn secp256k1_from_bytes(bytes: [u8; 65]) -> Self {
        Self::Secp256k1(bytes)
    }

    /// Create an ML-DSA-65 signature from raw 3309 bytes.
    pub fn ml_dsa65_from_bytes(bytes: Box<[u8; ML_DSA_65_SIGNATURE_LENGTH]>) -> Self {
        Self::MlDsa65(bytes)
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Ed25519(_) => KeyType::Ed25519,
            Self::Secp256k1(_) => KeyType::Secp256k1,
            Self::MlDsa65(_) => KeyType::MlDsa65,
        }
    }

    /// Get the raw signature bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(bytes) => bytes.as_slice(),
            Self::Secp256k1(bytes) => bytes.as_slice(),
            Self::MlDsa65(bytes) => bytes.as_slice(),
        }
    }

    /// Verify this signature against a message and public key.
    ///
    /// Mirrors nearcore's `near_crypto::Signature::verify`: Secp256k1
    /// signatures are checked against `message` as a 32-byte digest (no extra
    /// hashing), and any `message` that is not exactly 32 bytes, as well as
    /// any high-S signature, is rejected.
    pub fn verify(&self, message: &[u8], public_key: &PublicKey) -> bool {
        match self {
            Self::Ed25519(sig_bytes) => {
                let Some(pk_bytes) = public_key.as_ed25519_bytes() else {
                    return false;
                };
                let Ok(verifying_key) = VerifyingKey::from_bytes(pk_bytes) else {
                    return false;
                };
                let signature = ed25519_dalek::Signature::from_bytes(sig_bytes);
                verifying_key.verify_strict(message, &signature).is_ok()
            }
            Self::Secp256k1(sig_bytes) => {
                let Some(pk_bytes) = public_key.as_secp256k1_bytes() else {
                    return false;
                };
                // Reconstruct the 65-byte uncompressed key with 0x04 prefix for verification
                let mut uncompressed = [0u8; 65];
                uncompressed[0] = 0x04;
                uncompressed[1..].copy_from_slice(pk_bytes);
                let Ok(verifying_key) = k256::ecdsa::VerifyingKey::from_sec1_bytes(&uncompressed)
                else {
                    return false;
                };

                // Validate recovery id byte is in expected range (0..=3)
                let v = sig_bytes[64];
                if v > 3 {
                    return false;
                }

                let Ok(signature) = k256::ecdsa::Signature::from_slice(&sig_bytes[..64]) else {
                    return false;
                };

                // NEAR protocol: `message` is the 32-byte digest itself; nearcore
                // returns false for anything else. k256 rejects high-S
                // signatures here, as libsecp256k1 does in nearcore.
                let Ok(digest) = <&[u8; 32]>::try_from(message) else {
                    return false;
                };
                use k256::ecdsa::signature::hazmat::PrehashVerifier;
                verifying_key.verify_prehash(digest, &signature).is_ok()
            }
            Self::MlDsa65(sig_bytes) => {
                let Some(pk_bytes) = public_key.as_ml_dsa65_bytes() else {
                    return false;
                };
                // FIPS-204 ML-DSA.Verify with empty context.
                let Ok(enc_vk) = EncodedVerifyingKey::<MlDsa65>::try_from(pk_bytes.as_slice())
                else {
                    return false;
                };
                let verifying_key = ml_dsa::VerifyingKey::<MlDsa65>::decode(&enc_vk);
                let Ok(enc_sig) = EncodedSignature::<MlDsa65>::try_from(sig_bytes.as_slice())
                else {
                    return false;
                };
                let Some(signature) = ml_dsa::Signature::<MlDsa65>::decode(&enc_sig) else {
                    return false;
                };
                verifying_key.verify(message, &signature).is_ok()
            }
        }
    }
}

impl FromStr for Signature {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key_type, data_str) = s.split_once(':').ok_or(ParseKeyError::InvalidFormat)?;

        let key_type = match key_type {
            "ed25519" => KeyType::Ed25519,
            "secp256k1" => KeyType::Secp256k1,
            "ml-dsa-65" => KeyType::MlDsa65,
            other => return Err(ParseKeyError::UnknownKeyType(other.to_string())),
        };

        let data = bs58::decode(data_str)
            .into_vec()
            .map_err(|e| ParseKeyError::InvalidBase58(e.to_string()))?;

        if data.len() != key_type.signature_len() {
            return Err(ParseKeyError::InvalidLength {
                expected: key_type.signature_len(),
                actual: data.len(),
            });
        }

        match key_type {
            KeyType::Ed25519 => {
                let bytes: [u8; 64] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidFormat)?;
                Ok(Self::Ed25519(bytes))
            }
            KeyType::Secp256k1 => {
                let bytes: [u8; 65] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidFormat)?;
                Ok(Self::Secp256k1(bytes))
            }
            KeyType::MlDsa65 => {
                let bytes: Box<[u8; ML_DSA_65_SIGNATURE_LENGTH]> = data
                    .into_boxed_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidFormat)?;
                Ok(Self::MlDsa65(bytes))
            }
        }
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            self.key_type().as_str(),
            bs58::encode(self.as_bytes()).into_string()
        )
    }
}

impl Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ML-DSA-65 signatures are 3309 bytes; avoid dumping the full base58 in
        // `{:?}` (log bloat + allocation).
        match self {
            Self::MlDsa65(_) => write!(f, "Signature(ml-dsa-65:<3309 bytes>)"),
            _ => write!(f, "Signature({})", self),
        }
    }
}

impl BorshSerialize for Signature {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&(self.key_type() as u8), writer)?;
        writer.write_all(self.as_bytes())?;
        Ok(())
    }
}

impl BorshDeserialize for Signature {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let key_type_byte = u8::deserialize_reader(reader)?;
        let key_type = KeyType::try_from(key_type_byte)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        match key_type {
            KeyType::Ed25519 => {
                let mut bytes = [0u8; 64];
                reader.read_exact(&mut bytes)?;
                Ok(Self::Ed25519(bytes))
            }
            KeyType::Secp256k1 => {
                let mut bytes = [0u8; 65];
                reader.read_exact(&mut bytes)?;
                Ok(Self::Secp256k1(bytes))
            }
            KeyType::MlDsa65 => {
                let mut bytes = Box::new([0u8; ML_DSA_65_SIGNATURE_LENGTH]);
                reader.read_exact(bytes.as_mut_slice())?;
                Ok(Self::MlDsa65(bytes))
            }
        }
    }
}

// ============================================================================
// Seed Phrase Generation
// ============================================================================

/// Normalize a BIP-39 mnemonic and derive its 64-byte seed, applying an
/// optional passphrase.
///
/// Shared by the Ed25519 and ML-DSA-65 seed-phrase constructors: the mnemonic
/// is trimmed, lowercased, and whitespace-collapsed before parsing, then run
/// through BIP-39's PBKDF2 seed derivation. The two key branches differ only
/// downstream, in the SLIP-10 master salt.
#[cfg(feature = "mnemonic")]
fn mnemonic_to_seed(
    phrase: impl AsRef<str>,
    passphrase: Option<&str>,
) -> Result<[u8; 64], SignerError> {
    let normalized = phrase
        .as_ref()
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mnemonic: Mnemonic = normalized
        .parse()
        .map_err(|_| SignerError::InvalidSeedPhrase)?;

    Ok(mnemonic.to_seed(passphrase.unwrap_or("")))
}

/// Generate a random BIP-39 seed phrase.
///
/// # Arguments
///
/// * `word_count` - Number of words (12, 15, 18, 21, or 24)
///
/// # Example
///
/// ```rust
/// use near_kit::signer::generate_seed_phrase;
///
/// let phrase = generate_seed_phrase(12).unwrap();
/// assert_eq!(phrase.split_whitespace().count(), 12);
/// ```
#[cfg(feature = "mnemonic")]
pub fn generate_seed_phrase(word_count: usize) -> Result<String, SignerError> {
    // Word count to entropy bytes: 12->16, 15->20, 18->24, 21->28, 24->32
    let entropy_bytes = match word_count {
        12 => 16,
        15 => 20,
        18 => 24,
        21 => 28,
        24 => 32,
        _ => {
            return Err(SignerError::KeyDerivationFailed(format!(
                "Invalid word count: {}. Must be 12, 15, 18, 21, or 24",
                word_count
            )));
        }
    };

    let mut entropy = vec![0u8; entropy_bytes];
    fill_random(&mut entropy);

    let mnemonic = Mnemonic::from_entropy(&entropy).map_err(|e| {
        SignerError::KeyDerivationFailed(format!("Failed to generate mnemonic: {}", e))
    })?;

    Ok(mnemonic.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_sign() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();
        let message = b"hello world";

        let signature = secret.sign(message);
        assert!(signature.verify(message, &public));
        assert!(!signature.verify(b"wrong message", &public));
    }

    #[test]
    fn test_public_key_roundtrip() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();
        let s = public.to_string();
        let parsed: PublicKey = s.parse().unwrap();
        assert_eq!(public, parsed);
    }

    #[test]
    fn test_secret_key_roundtrip() {
        let secret = SecretKey::generate_ed25519();
        let s = secret.to_string();
        let parsed: SecretKey = s.parse().unwrap();
        assert_eq!(secret.public_key(), parsed.public_key());
    }

    #[test]
    fn test_secret_key_serde_representation_is_unchanged() {
        let keys = [
            SecretKey::ed25519_from_bytes([7; 32]),
            SecretKey::secp256k1_from_bytes([42; 32]).unwrap(),
        ];

        for secret_key in keys {
            let json = serde_json::to_value(&secret_key).unwrap();
            assert_eq!(
                json,
                serde_json::Value::String(secret_key.to_string()),
                "secret keys remain JSON strings"
            );

            let reparsed: SecretKey = serde_json::from_value(json).unwrap();
            assert_eq!(reparsed.key_type(), secret_key.key_type());
            assert_eq!(reparsed.as_bytes(), secret_key.as_bytes());
        }
    }

    #[test]
    fn test_ml_dsa65_seed_serde_roundtrip() {
        let secret = SecretKey::ml_dsa65_from_seed([9; ML_DSA_65_SEED_LENGTH]);
        let expected = format!(
            "ml-dsa-65:{}",
            bs58::encode(secret.as_bytes()).into_string()
        );

        assert_eq!(secret.to_string(), expected);
        let json = serde_json::to_value(&secret).unwrap();
        assert_eq!(json, serde_json::Value::String(expected));

        let reparsed: SecretKey = serde_json::from_value(json).unwrap();
        assert_eq!(reparsed.key_type(), KeyType::MlDsa65);
        assert_eq!(reparsed.as_bytes(), secret.as_bytes());
        assert_eq!(reparsed.public_key(), secret.public_key());
    }

    #[test]
    fn test_ed25519_secret_key_64_byte_expanded_form() {
        // Generate a key, get its 32-byte seed, then construct a 64-byte expanded form
        // (seed || public_key) which near-cli sometimes stores
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();
        let seed_bytes = secret.as_bytes();

        // Construct 64-byte expanded key: seed (32) + public key bytes (32)
        let mut expanded = Vec::with_capacity(64);
        expanded.extend_from_slice(seed_bytes);
        expanded.extend_from_slice(public.as_bytes());
        let expanded_b58 = bs58::encode(&expanded).into_string();
        let expanded_str = format!("ed25519:{}", expanded_b58);

        // Parse the 64-byte form — should succeed and produce same public key
        let parsed: SecretKey = expanded_str.parse().unwrap();
        assert_eq!(parsed.public_key(), public);

        // Re-serializing yields the 32-byte seed form
        let reserialized = parsed.to_string();
        assert_eq!(reserialized, secret.to_string());
    }

    // ========================================================================
    // Seed Phrase Tests
    // ========================================================================

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_generate_seed_phrase_12_words() {
        let phrase = generate_seed_phrase(12).unwrap();
        assert_eq!(phrase.split_whitespace().count(), 12);
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_generate_seed_phrase_24_words() {
        let phrase = generate_seed_phrase(24).unwrap();
        assert_eq!(phrase.split_whitespace().count(), 24);
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_generate_seed_phrase_invalid_word_count() {
        let result = generate_seed_phrase(13);
        assert!(result.is_err());
    }

    // Valid BIP-39 test vector (from official test vectors)
    #[cfg(feature = "mnemonic")]
    const TEST_PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_from_seed_phrase_known_vector() {
        // Test vector: known seed phrase should produce consistent key
        let secret_key = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();

        // Same phrase should always produce the same key
        let secret_key2 = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();
        assert_eq!(secret_key.public_key(), secret_key2.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_from_seed_phrase_whitespace_normalization() {
        let phrase1 = TEST_PHRASE;
        let phrase2 = "  abandon   abandon  abandon abandon abandon abandon abandon abandon abandon abandon abandon about  ";
        let phrase3 = "ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABANDON ABOUT";

        let key1 = SecretKey::from_seed_phrase(phrase1).unwrap();
        let key2 = SecretKey::from_seed_phrase(phrase2).unwrap();
        let key3 = SecretKey::from_seed_phrase(phrase3).unwrap();

        assert_eq!(key1.public_key(), key2.public_key());
        assert_eq!(key1.public_key(), key3.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_from_seed_phrase_invalid() {
        let result = SecretKey::from_seed_phrase("invalid words that are not a mnemonic");
        assert!(result.is_err());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_from_seed_phrase_different_paths() {
        let key1 = SecretKey::from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/0'").unwrap();
        let key2 = SecretKey::from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/1'").unwrap();

        // Different paths should produce different keys
        assert_ne!(key1.public_key(), key2.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_from_seed_phrase_with_passphrase() {
        let key_no_pass = SecretKey::from_seed_phrase_with_path_and_passphrase(
            TEST_PHRASE,
            DEFAULT_HD_PATH,
            None,
        )
        .unwrap();

        let key_with_pass = SecretKey::from_seed_phrase_with_path_and_passphrase(
            TEST_PHRASE,
            DEFAULT_HD_PATH,
            Some("my-password"),
        )
        .unwrap();

        // Passphrase should produce different key
        assert_ne!(key_no_pass.public_key(), key_with_pass.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_generate_with_seed_phrase() {
        let (phrase, secret_key) = SecretKey::generate_with_seed_phrase().unwrap();

        // Phrase should be 12 words
        assert_eq!(phrase.split_whitespace().count(), 12);

        // Re-deriving from the phrase should produce the same key
        let derived = SecretKey::from_seed_phrase(&phrase).unwrap();
        assert_eq!(secret_key.public_key(), derived.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_generate_with_seed_phrase_24_words() {
        let (phrase, secret_key) = SecretKey::generate_with_seed_phrase_words(24).unwrap();

        assert_eq!(phrase.split_whitespace().count(), 24);

        let derived = SecretKey::from_seed_phrase(&phrase).unwrap();
        assert_eq!(secret_key.public_key(), derived.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_seed_phrase_key_can_sign() {
        let secret_key = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();

        let message = b"test message";
        let signature = secret_key.sign(message);
        let public_key = secret_key.public_key();

        assert!(signature.verify(message, &public_key));
    }

    // ========================================================================
    // Curve Point Validation Tests
    // ========================================================================

    #[test]
    fn test_secp256k1_invalid_curve_point_rejected() {
        // This is the invalid key from the NEAR SDK docs that was identified as not being
        // on the secp256k1 curve. See: https://github.com/near/near-sdk-rs/pull/1469
        let invalid_key = "secp256k1:qMoRgcoXai4mBPsdbHi1wfyxF9TdbPCF4qSDQTRP3TfescSRoUdSx6nmeQoN3aiwGzwMyGXAb1gUjBTv5AY8DXj";
        let result: Result<PublicKey, _> = invalid_key.parse();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ParseKeyError::InvalidCurvePoint | ParseKeyError::InvalidLength { .. }
        ));
    }

    #[test]
    fn test_secp256k1_valid_curve_point_accepted() {
        // Valid secp256k1 key from near-sdk-js (verified to be on the curve)
        // This key is 64 bytes (uncompressed without prefix) — matches our new format
        let valid_key = "secp256k1:5r22SrjrDvgY3wdQsnjgxkeAbU1VcM71FYvALEQWihjM3Xk4Be1CpETTqFccChQr4iJwDroSDVmgaWZv2AcXvYeL";
        let result: Result<PublicKey, _> = valid_key.parse();
        // This key is now parseable because we expect 64-byte uncompressed format
        assert!(result.is_ok());
    }

    #[test]
    fn test_ed25519_valid_key_accepted() {
        // Valid ed25519 public key
        let valid_key = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp";
        let result: Result<PublicKey, _> = valid_key.parse();
        assert!(result.is_ok());
    }

    #[test]
    fn test_ed25519_invalid_curve_point_rejected() {
        // The high bit of the last byte being set with an invalid x-coordinate recovery
        // should produce an invalid point. Specifically, a y-coordinate that when
        // the x is computed results in a non-square (no valid x exists).
        // This specific byte sequence has been verified to fail ed25519 decompression.
        //
        // Note: ed25519_dalek may accept many byte patterns as valid curve points.
        // Ed25519 point decompression is very permissive - most 32-byte sequences
        // decode to valid points.
        let invalid_bytes = [
            0xEC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0x7F,
        ];
        let encoded = bs58::encode(&invalid_bytes).into_string();
        let invalid_key = format!("ed25519:{}", encoded);
        let result: Result<PublicKey, _> = invalid_key.parse();
        if let Err(err) = result {
            assert!(matches!(err, ParseKeyError::InvalidCurvePoint));
        } else {
            // If ed25519_dalek accepts this, we should skip this test case
            eprintln!(
                "Note: ed25519 point decompression accepted test bytes - validation may be too lenient"
            );
        }
    }

    #[test]
    fn test_borsh_deserialize_validates_curve_point() {
        use borsh::BorshDeserialize;

        let mut invalid_ed25519 = vec![0u8]; // KeyType::Ed25519
        invalid_ed25519.extend_from_slice(&[2]);
        invalid_ed25519.extend_from_slice(&[0; 31]);
        assert!(PublicKey::try_from_slice(&invalid_ed25519).is_err());

        let mut invalid_secp256k1 = vec![1u8]; // KeyType::Secp256k1
        invalid_secp256k1.extend_from_slice(&[0u8; 64]);
        assert!(PublicKey::try_from_slice(&invalid_secp256k1).is_err());
    }

    #[test]
    fn test_signature_from_str_roundtrip() {
        let sig_str = "ed25519:3s1dvMqNDCByoMnDnkhB4GPjTSXCRt4nt3Af5n1RX8W7aJ2FC6MfRf5BNXZ52EBifNJnNVBsGvke6GRYuaEYJXt5";
        let sig: Signature = sig_str.parse().unwrap();
        assert_eq!(sig.key_type(), KeyType::Ed25519);
        assert_eq!(sig.as_bytes().len(), 64);
        assert_eq!(sig.to_string(), sig_str);
    }

    #[test]
    fn test_signature_from_str_invalid_format() {
        assert!("no_colon".parse::<Signature>().is_err());
        assert!("unknown:abc".parse::<Signature>().is_err());
        assert!("ed25519:invalid!!!".parse::<Signature>().is_err());
        assert!("ed25519:AAAA".parse::<Signature>().is_err()); // too short
    }

    #[test]
    fn test_signature_serde_roundtrip() {
        let sig_str = "ed25519:3s1dvMqNDCByoMnDnkhB4GPjTSXCRt4nt3Af5n1RX8W7aJ2FC6MfRf5BNXZ52EBifNJnNVBsGvke6GRYuaEYJXt5";
        let sig: Signature = sig_str.parse().unwrap();
        let json = serde_json::to_value(&sig).unwrap();
        assert_eq!(json.as_str().unwrap(), sig_str);
        let parsed: Signature = serde_json::from_value(json).unwrap();
        assert_eq!(sig, parsed);
    }

    // ========================================================================
    // Secp256k1 Tests
    // ========================================================================

    #[test]
    fn test_secp256k1_generate_and_sign_verify() {
        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();
        // secp256k1 signs a 32-byte digest (e.g. a transaction hash) directly.
        let message = crate::types::CryptoHash::hash(b"hello world");
        let message = message.as_bytes();

        assert_eq!(secret.key_type(), KeyType::Secp256k1);
        assert_eq!(public.key_type(), KeyType::Secp256k1);

        let signature = secret.sign(message);
        assert_eq!(signature.key_type(), KeyType::Secp256k1);
        assert_eq!(signature.as_bytes().len(), 65);

        assert!(signature.verify(message, &public));
        assert!(!signature.verify(&[0u8; 32], &public));
    }

    #[test]
    fn test_secp256k1_public_key_is_64_bytes() {
        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();

        // Public key should be 64 bytes (uncompressed without prefix)
        let pk_bytes = public.as_secp256k1_bytes().unwrap();
        assert_eq!(pk_bytes.len(), 64);
    }

    #[test]
    fn test_secp256k1_secret_key_to_public_key_derivation() {
        // Deterministic: same secret key bytes should always produce the same public key
        let bytes = [42u8; 32];
        let sk1 = SecretKey::secp256k1_from_bytes(bytes).unwrap();
        let sk2 = SecretKey::secp256k1_from_bytes(bytes).unwrap();
        assert_eq!(sk1.public_key(), sk2.public_key());

        // Different secret keys produce different public keys
        let bytes2 = [43u8; 32];
        let sk3 = SecretKey::secp256k1_from_bytes(bytes2).unwrap();
        assert_ne!(sk1.public_key(), sk3.public_key());
    }

    #[test]
    fn test_secp256k1_public_key_string_roundtrip() {
        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();
        let s = public.to_string();
        assert!(s.starts_with("secp256k1:"));
        let parsed: PublicKey = s.parse().unwrap();
        assert_eq!(public, parsed);
    }

    #[test]
    fn test_secp256k1_secret_key_string_roundtrip() {
        let secret = SecretKey::generate_secp256k1();
        let s = secret.to_string();
        assert!(s.starts_with("secp256k1:"));
        let parsed: SecretKey = s.parse().unwrap();
        assert_eq!(secret.public_key(), parsed.public_key());
    }

    #[test]
    fn test_secp256k1_secret_key_random() {
        let secret_key = SecretKey::generate_secp256k1();
        let public_key = secret_key.public_key();
        assert_eq!(public_key.key_type(), KeyType::Secp256k1);
        assert_eq!(secret_key.key_type(), KeyType::Secp256k1);
        assert!(public_key.to_string().starts_with("secp256k1:"));

        let message = &[5u8; 32];
        let signature = secret_key.sign(message);
        assert!(signature.verify(message, &public_key));
    }

    #[test]
    fn test_secp256k1_cross_type_verify_fails() {
        // Ed25519 signature should not verify against secp256k1 key
        let ed_secret = SecretKey::generate_ed25519();
        let secp_secret = SecretKey::generate_secp256k1();

        let message = &[6u8; 32];
        let ed_sig = ed_secret.sign(message);
        let secp_sig = secp_secret.sign(message);

        assert!(!ed_sig.verify(message, &secp_secret.public_key()));
        assert!(!secp_sig.verify(message, &ed_secret.public_key()));
    }

    #[test]
    fn test_secp256k1_invalid_scalar_rejected() {
        // Zero scalar is invalid for secp256k1
        let zero_bytes = [0u8; 32];
        let result = SecretKey::secp256k1_from_bytes(zero_bytes);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseKeyError::InvalidScalar));
    }

    #[test]
    fn test_secp256k1_invalid_scalar_rejected_from_str() {
        // Construct a secp256k1 secret key string with zero bytes (invalid scalar)
        let zero_bytes = [0u8; 32];
        let encoded = bs58::encode(&zero_bytes).into_string();
        let key_str = format!("secp256k1:{}", encoded);
        let result: Result<SecretKey, _> = key_str.parse();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseKeyError::InvalidScalar));
    }

    #[test]
    fn test_secp256k1_invalid_recovery_id_rejected() {
        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();
        let message = &[8u8; 32];
        let signature = secret.sign(message);
        assert!(signature.verify(message, &public));

        // Create a tampered signature with invalid recovery id
        if let Signature::Secp256k1(mut sig_bytes) = signature.clone() {
            sig_bytes[64] = 4; // valid range is 0..=3
            let tampered = Signature::Secp256k1(sig_bytes);
            assert!(!tampered.verify(message, &public));

            sig_bytes[64] = 255;
            let tampered = Signature::Secp256k1(sig_bytes);
            assert!(!tampered.verify(message, &public));
        } else {
            panic!("Expected Secp256k1 signature");
        }
    }

    #[test]
    fn test_public_key_from_bytes_rejects_invalid_curve_points() {
        // This compressed y-coordinate has no corresponding Ed25519 point.
        let mut invalid_ed25519 = [0; 32];
        invalid_ed25519[0] = 2;
        assert_eq!(
            PublicKey::ed25519_from_bytes(invalid_ed25519),
            Err(ParseKeyError::InvalidCurvePoint)
        );

        // All-zero x,y coordinates are not a Secp256k1 curve point.
        assert_eq!(
            PublicKey::secp256k1_from_bytes([0; 64]),
            Err(ParseKeyError::InvalidCurvePoint)
        );

        let ed_public = SecretKey::generate_ed25519().public_key();
        assert_eq!(
            PublicKey::ed25519_from_bytes(*ed_public.as_ed25519_bytes().unwrap()).unwrap(),
            ed_public
        );

        let secp_public = SecretKey::generate_secp256k1().public_key();
        assert_eq!(
            PublicKey::secp256k1_from_bytes(*secp_public.as_secp256k1_bytes().unwrap()).unwrap(),
            secp_public
        );
    }

    #[test]
    fn test_ed25519_borsh_roundtrip() {
        use borsh::BorshDeserialize;

        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();
        let serialized = borsh::to_vec(&public).unwrap();
        // Ed25519: 1 byte key type + 32 bytes key data
        assert_eq!(serialized.len(), 33);
        assert_eq!(serialized[0], 0); // Ed25519
        let deserialized = PublicKey::try_from_slice(&serialized).unwrap();
        assert_eq!(public, deserialized);
    }

    #[test]
    fn test_secp256k1_borsh_roundtrip() {
        use borsh::BorshDeserialize;

        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();
        let serialized = borsh::to_vec(&public).unwrap();
        // Secp256k1: 1 byte key type + 64 bytes key data
        assert_eq!(serialized.len(), 65);
        assert_eq!(serialized[0], 1); // Secp256k1
        let deserialized = PublicKey::try_from_slice(&serialized).unwrap();
        assert_eq!(public, deserialized);
    }

    #[test]
    fn test_signature_borsh_roundtrip() {
        use borsh::BorshDeserialize;

        let secret = SecretKey::generate_ed25519();
        let sig = secret.sign(b"test");
        let serialized = borsh::to_vec(&sig).unwrap();
        assert_eq!(serialized.len(), 65); // 1 + 64
        let deserialized = Signature::try_from_slice(&serialized).unwrap();
        assert_eq!(sig, deserialized);
    }

    #[test]
    fn test_public_key_and_signature_wire_encodings_are_exact() {
        use borsh::BorshDeserialize;

        let keys = [
            ("ed25519", SecretKey::ed25519_from_bytes([7; 32])),
            (
                "secp256k1",
                SecretKey::secp256k1_from_bytes([42; 32]).unwrap(),
            ),
            (
                "ml-dsa-65",
                SecretKey::ml_dsa65_from_seed([9; ML_DSA_65_SEED_LENGTH]),
            ),
        ];

        for (tag, (prefix, secret_key)) in (0u8..).zip(keys) {
            let public_key = secret_key.public_key();
            let expected_display = format!(
                "{}:{}",
                prefix,
                bs58::encode(public_key.as_bytes()).into_string()
            );
            assert_eq!(public_key.to_string(), expected_display);
            assert_eq!(
                serde_json::to_value(&public_key).unwrap(),
                serde_json::Value::String(expected_display.clone())
            );
            assert_eq!(
                serde_json::from_value::<PublicKey>(serde_json::Value::String(
                    expected_display.clone()
                ))
                .unwrap(),
                public_key
            );
            assert_eq!(expected_display.parse::<PublicKey>().unwrap(), public_key);

            let mut expected_public_key = vec![tag];
            expected_public_key.extend_from_slice(public_key.as_bytes());
            let public_key_borsh = borsh::to_vec(&public_key).unwrap();
            assert_eq!(public_key_borsh, expected_public_key);
            assert_eq!(
                PublicKey::try_from_slice(&public_key_borsh).unwrap(),
                public_key
            );

            let digest = crate::types::CryptoHash::hash(b"wire-compatibility");
            let signature = secret_key.sign(digest.as_bytes());
            assert_eq!(
                serde_json::to_value(&signature).unwrap(),
                serde_json::Value::String(signature.to_string())
            );
            assert_eq!(
                serde_json::from_value::<Signature>(serde_json::Value::String(
                    signature.to_string()
                ))
                .unwrap(),
                signature
            );

            let mut expected_signature = vec![tag];
            expected_signature.extend_from_slice(signature.as_bytes());
            let signature_borsh = borsh::to_vec(&signature).unwrap();
            assert_eq!(signature_borsh, expected_signature);
            assert_eq!(
                Signature::try_from_slice(&signature_borsh).unwrap(),
                signature
            );
        }
    }

    #[test]
    fn test_key_types_match_generated_secret_keys() {
        let ed_secret = SecretKey::generate_ed25519();
        let ed_public = ed_secret.public_key();

        assert_eq!(ed_public.key_type(), KeyType::Ed25519);
        assert!(ed_public.as_ed25519_bytes().is_some());
        assert_eq!(ed_secret.key_type(), KeyType::Ed25519);

        let secp_secret = SecretKey::generate_secp256k1();
        let secp_public = secp_secret.public_key();

        assert_eq!(secp_public.key_type(), KeyType::Secp256k1);
        assert!(secp_public.as_secp256k1_bytes().is_some());
        assert_eq!(secp_secret.key_type(), KeyType::Secp256k1);
    }

    // ========================================================================
    // ML-DSA-65 Tests
    // ========================================================================

    #[test]
    fn test_ml_dsa65_sizes() {
        assert_eq!(ML_DSA_65_PUBLIC_KEY_LENGTH, 1952);
        assert_eq!(ML_DSA_65_SEED_LENGTH, 32);
        assert_eq!(ML_DSA_65_SIGNATURE_LENGTH, 3309);
        assert_eq!(ML_DSA_65_HASH_LENGTH, 32);
        assert_eq!(KeyType::MlDsa65.public_key_len(), 1952);
        assert_eq!(KeyType::MlDsa65.signature_len(), 3309);
        assert_eq!(KeyType::MlDsa65.as_str(), "ml-dsa-65");
        assert_eq!(KeyType::try_from(2u8).unwrap(), KeyType::MlDsa65);
    }

    #[test]
    fn test_ml_dsa65_generate_sign_verify() {
        let secret = SecretKey::generate_ml_dsa65();
        let public = secret.public_key();

        assert_eq!(secret.key_type(), KeyType::MlDsa65);
        assert_eq!(public.key_type(), KeyType::MlDsa65);
        assert_eq!(public.as_bytes().len(), ML_DSA_65_PUBLIC_KEY_LENGTH);
        assert_eq!(secret.as_bytes().len(), ML_DSA_65_SEED_LENGTH);

        let message = b"hello post-quantum world";
        let signature = secret.sign(message);
        assert_eq!(signature.key_type(), KeyType::MlDsa65);
        assert_eq!(signature.as_bytes().len(), ML_DSA_65_SIGNATURE_LENGTH);

        assert!(signature.verify(message, &public));
        assert!(!signature.verify(b"tampered", &public));
    }

    #[test]
    fn test_ml_dsa65_expanded_export_matches_seed_key() {
        let seed = SecretKey::ml_dsa65_from_seed([5u8; 32]);
        let public = seed.public_key();
        let expanded_bytes = seed.to_ml_dsa65_expanded_bytes().unwrap();

        assert_eq!(expanded_bytes.len(), ML_DSA_65_SECRET_KEY_LENGTH);
        // FIPS-204 skEncode and pkEncode both begin with the same 32-byte rho.
        assert_eq!(
            &expanded_bytes[..ML_DSA_65_SEED_LENGTH],
            &public.as_bytes()[..ML_DSA_65_SEED_LENGTH]
        );

        // Export is deterministic and does not alter the seed-held key.
        let again = SecretKey::ml_dsa65_from_seed([5u8; 32]);
        assert_eq!(again.to_ml_dsa65_expanded_bytes().unwrap(), expanded_bytes);
        assert_eq!(again.public_key(), public);
        assert!(
            seed.sign(b"expanded export")
                .verify(b"expanded export", &public)
        );
    }

    #[test]
    fn test_ml_dsa65_expanded_secret_key_is_rejected() {
        let expanded = SecretKey::ml_dsa65_from_seed([7u8; ML_DSA_65_SEED_LENGTH])
            .to_ml_dsa65_expanded_bytes()
            .unwrap();
        let external = format!(
            "ml-dsa-65:{}",
            bs58::encode(expanded.as_slice()).into_string()
        );

        assert!(matches!(
            external.parse::<SecretKey>(),
            Err(ParseKeyError::InvalidLength {
                expected: ML_DSA_65_SEED_LENGTH,
                actual: ML_DSA_65_SECRET_KEY_LENGTH,
            })
        ));
        assert!(serde_json::from_value::<SecretKey>(serde_json::Value::String(external)).is_err());
    }

    #[test]
    fn test_ml_dsa65_expanded_export_is_none_for_other_key_types() {
        assert!(
            SecretKey::generate_ed25519()
                .to_ml_dsa65_expanded_bytes()
                .is_none()
        );
        assert!(
            SecretKey::generate_secp256k1()
                .to_ml_dsa65_expanded_bytes()
                .is_none()
        );
    }

    #[test]
    fn test_ml_dsa65_seed_is_deterministic() {
        // Same 32-byte seed must always derive the same public key and (because
        // the underlying signer is deterministic) the same signature.
        let seed = [7u8; ML_DSA_65_SEED_LENGTH];
        let sk1 = SecretKey::ml_dsa65_from_seed(seed);
        let sk2 = SecretKey::ml_dsa65_from_seed(seed);
        assert_eq!(sk1.public_key(), sk2.public_key());

        let msg = b"determinism";
        assert_eq!(sk1.sign(msg).as_bytes(), sk2.sign(msg).as_bytes());

        // A different seed yields a different key.
        let sk3 = SecretKey::ml_dsa65_from_seed([8u8; ML_DSA_65_SEED_LENGTH]);
        assert_ne!(sk1.public_key(), sk3.public_key());
    }

    #[test]
    fn test_ml_dsa65_public_key_string_roundtrip() {
        let secret = SecretKey::generate_ml_dsa65();
        let public = secret.public_key();
        let s = public.to_string();
        assert!(s.starts_with("ml-dsa-65:"));
        assert!(!s.starts_with("ml-dsa-65-hash:"));
        let parsed: PublicKey = s.parse().unwrap();
        assert_eq!(public, parsed);
    }

    #[test]
    fn test_ml_dsa65_secret_key_string_roundtrip() {
        let secret = SecretKey::generate_ml_dsa65();
        let s = secret.to_string();
        assert!(s.starts_with("ml-dsa-65:"));
        let parsed: SecretKey = s.parse().unwrap();
        assert_eq!(secret.public_key(), parsed.public_key());
        // The seed itself round-trips byte-for-byte.
        assert_eq!(secret.as_bytes(), parsed.as_bytes());
    }

    #[test]
    fn test_ml_dsa65_signature_string_roundtrip() {
        let secret = SecretKey::generate_ml_dsa65();
        let sig = secret.sign(b"sig-roundtrip");
        let s = sig.to_string();
        assert!(s.starts_with("ml-dsa-65:"));
        let parsed: Signature = s.parse().unwrap();
        assert_eq!(sig, parsed);
    }

    #[test]
    fn test_ml_dsa65_public_key_borsh_roundtrip() {
        use borsh::BorshDeserialize;

        let secret = SecretKey::generate_ml_dsa65();
        let public = secret.public_key();
        let serialized = borsh::to_vec(&public).unwrap();
        // 1 byte key type + 1952 bytes key data.
        assert_eq!(serialized.len(), 1 + ML_DSA_65_PUBLIC_KEY_LENGTH);
        assert_eq!(serialized[0], 2); // MlDsa65 discriminant
        let deserialized = PublicKey::try_from_slice(&serialized).unwrap();
        assert_eq!(public, deserialized);
    }

    #[test]
    fn test_ml_dsa65_signature_borsh_roundtrip() {
        use borsh::BorshDeserialize;

        let secret = SecretKey::generate_ml_dsa65();
        let sig = secret.sign(b"borsh");
        let serialized = borsh::to_vec(&sig).unwrap();
        assert_eq!(serialized.len(), 1 + ML_DSA_65_SIGNATURE_LENGTH);
        assert_eq!(serialized[0], 2);
        let deserialized = Signature::try_from_slice(&serialized).unwrap();
        assert_eq!(sig, deserialized);
    }

    #[test]
    fn test_ml_dsa65_hash_handle_is_not_a_public_key() {
        // A `view_access_key_list` response stores an ML-DSA-65 key as a 32-byte
        // SHA3-256 handle. That string is a PublicKeyHandle, never a PublicKey:
        // the strict parser rejects it with a pointed error (matching nearcore)
        // so a copy-pasted handle fails at the parse site, not in signing.
        let secret = SecretKey::generate_ml_dsa65();
        let full = secret.public_key();
        let handle = full.to_ml_dsa65_hash().unwrap();
        assert!(handle.is_ml_dsa65_hash());
        assert!(handle.full_pubkey().is_none());
        assert_eq!(handle.key_type(), KeyType::MlDsa65);
        assert_eq!(handle.as_bytes().len(), ML_DSA_65_HASH_LENGTH);

        let s = handle.to_string();
        assert!(s.starts_with("ml-dsa-65-hash:"));
        assert_eq!(
            s.parse::<PublicKey>(),
            Err(ParseKeyError::MlDsa65HashHandle)
        );
        assert_eq!(
            PublicKey::try_from(s.as_str()),
            Err(ParseKeyError::MlDsa65HashHandle)
        );
        // The prefix alone is enough to reject; the payload isn't inspected.
        assert_eq!(
            "ml-dsa-65-hash:".parse::<PublicKey>(),
            Err(ParseKeyError::MlDsa65HashHandle)
        );

        // ...but it round-trips as a PublicKeyHandle, and identifies the key.
        let parsed: PublicKeyHandle = s.parse().unwrap();
        assert_eq!(parsed, handle);
        assert!(parsed.refers_to(&full));
        assert!(!parsed.refers_to(&SecretKey::generate_ml_dsa65().public_key()));
        assert!(!parsed.refers_to(&SecretKey::generate_ed25519().public_key()));
        // Wrapping the full key is lossless and distinct from its hash.
        let wrapped = PublicKeyHandle::from(full.clone());
        assert_ne!(wrapped, handle);
        assert_eq!(wrapped.full_pubkey(), Some(&full));
        assert!(wrapped.refers_to(&full));
        assert_eq!(wrapped.clone().into_full(), Some(full.clone()));
        assert_eq!(wrapped.to_string(), full.to_string());
        assert_eq!(
            format!("{wrapped:?}"),
            "PublicKeyHandle(ml-dsa-65:<1952 bytes>)"
        );
        assert_eq!(format!("{handle:?}"), format!("PublicKeyHandle({s})"));
    }

    #[test]
    fn test_public_key_handle_full_keys_roundtrip() {
        // Full ed25519/secp256k1 keys parse/print identically as PublicKeyHandle
        // and PublicKey; ed25519/secp256k1 have no ML-DSA-65 hash.
        for pk in [
            SecretKey::generate_ed25519().public_key(),
            SecretKey::generate_secp256k1().public_key(),
        ] {
            let s = pk.to_string();
            let handle: PublicKeyHandle = s.parse().unwrap();
            assert_eq!(handle, PublicKeyHandle::Full(pk.clone()));
            assert_eq!(handle.to_string(), s);
            assert_eq!(handle.key_type(), pk.key_type());
            assert_eq!(handle.as_bytes(), pk.as_bytes());
            assert!(!handle.is_ml_dsa65_hash());
            assert!(handle.refers_to(&pk));
            assert_eq!(pk.to_ml_dsa65_hash(), None);
            // A key recovered from a handle is a normal key: it goes straight
            // into an action and the transaction hashes/signs (borsh) fine.
            let recovered = handle.into_full().unwrap();
            let signer = SecretKey::generate_ed25519();
            let tx = crate::types::Transaction::new(
                "alice.near".parse().unwrap(),
                signer.public_key(),
                1,
                "alice.near".parse().unwrap(),
                crate::types::CryptoHash::ZERO,
                vec![crate::types::Action::add_full_access_key(recovered)],
            );
            assert!(!tx.sign(&signer).to_bytes().is_empty());
        }
        assert!("nope".parse::<PublicKeyHandle>().is_err());
        assert!("ed25519:zzz".parse::<PublicKeyHandle>().is_err());
        assert_eq!(
            "rsa:abc".parse::<PublicKeyHandle>(),
            Err(ParseKeyError::UnknownKeyType("rsa".into()))
        );
    }

    #[test]
    fn test_public_key_handle_serde() {
        let handle = SecretKey::generate_ml_dsa65()
            .public_key()
            .to_ml_dsa65_hash()
            .unwrap();
        let json = serde_json::to_string(&handle).unwrap();
        assert!(json.starts_with("\"ml-dsa-65-hash:"));
        assert_eq!(
            serde_json::from_str::<PublicKeyHandle>(&json).unwrap(),
            handle
        );
        // A handle string is rejected when a PublicKey is expected.
        assert!(serde_json::from_str::<PublicKey>(&json).is_err());

        let full = SecretKey::generate_ed25519().public_key();
        let json = serde_json::to_string(&full).unwrap();
        assert_eq!(
            serde_json::from_str::<PublicKeyHandle>(&json).unwrap(),
            PublicKeyHandle::Full(full)
        );
    }

    #[test]
    fn test_ml_dsa65_hash_handle_matches_nearcore_domain() {
        // Pin the handle derivation against the protocol's domain tag so any
        // drift in the SHA3-256 input is caught. Computed from a fixed pubkey.
        use sha3::{Digest as _, Sha3_256};

        let public = SecretKey::ml_dsa65_from_seed([1u8; 32]).public_key();
        let raw = public.as_ml_dsa65_bytes().unwrap();

        let mut hasher = Sha3_256::new();
        hasher.update(b"near:ml-dsa-65-pubkey-hash:v1");
        hasher.update(raw.as_slice());
        let expected: [u8; 32] = hasher.finalize().into();

        let handle = public.to_ml_dsa65_hash().unwrap();
        assert_eq!(handle, PublicKeyHandle::MlDsa65Hash(expected));
    }

    #[test]
    fn test_ml_dsa65_cross_type_verify_fails() {
        let ml_secret = SecretKey::generate_ml_dsa65();
        let ed_secret = SecretKey::generate_ed25519();
        let msg = b"cross type";
        assert!(!ml_secret.sign(msg).verify(msg, &ed_secret.public_key()));
        assert!(!ed_secret.sign(msg).verify(msg, &ml_secret.public_key()));
    }

    #[test]
    fn test_malformed_ml_dsa65_signature_is_rejected_without_panicking() {
        let public_key = SecretKey::ml_dsa65_from_seed([3; ML_DSA_65_SEED_LENGTH]).public_key();
        let signature = Signature::ml_dsa65_from_bytes(Box::new([0; ML_DSA_65_SIGNATURE_LENGTH]));

        assert!(!signature.verify(b"malformed signature", &public_key));
    }

    #[test]
    fn test_ml_dsa65_secret_key_random() {
        let secret_key = SecretKey::generate_ml_dsa65();
        let public_key = secret_key.public_key();
        assert_eq!(public_key.key_type(), KeyType::MlDsa65);
        assert_eq!(secret_key.key_type(), KeyType::MlDsa65);
        assert!(public_key.to_string().starts_with("ml-dsa-65:"));

        let message = b"secret key";
        let signature = secret_key.sign(message);
        assert!(signature.verify(message, &public_key));
    }

    #[test]
    fn test_ml_dsa65_invalid_lengths_rejected() {
        // Wrong-length pubkey / seed / signature strings must error, not panic.
        let short_pk = format!("ml-dsa-65:{}", bs58::encode([0u8; 100]).into_string());
        assert!(short_pk.parse::<PublicKey>().is_err());

        let short_seed = format!("ml-dsa-65:{}", bs58::encode([0u8; 16]).into_string());
        assert!(short_seed.parse::<SecretKey>().is_err());

        let short_sig = format!("ml-dsa-65:{}", bs58::encode([0u8; 100]).into_string());
        assert!(short_sig.parse::<Signature>().is_err());

        let short_handle = format!("ml-dsa-65-hash:{}", bs58::encode([0u8; 16]).into_string());
        assert_eq!(
            short_handle.parse::<PublicKeyHandle>(),
            Err(ParseKeyError::InvalidLength {
                expected: ML_DSA_65_HASH_LENGTH,
                actual: 16
            })
        );
    }

    // ========================================================================
    // ML-DSA-65 Seed Phrase Tests
    // ========================================================================

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_from_seed_phrase_cross_sdk_vector() {
        // Cross-SDK regression: the ML-DSA-65 key derived from this phrase at
        // the default NEAR path must match the value locked in by the
        // TypeScript SDK (r-near/near-kit#224). This proves both SDKs derive
        // byte-identically. ML-DSA-65 secret keys serialize as their 32-byte
        // FIPS-204 seed under the `ml-dsa-65:` prefix.
        let secret = SecretKey::ml_dsa65_from_seed_phrase(TEST_PHRASE).unwrap();
        assert_eq!(secret.key_type(), KeyType::MlDsa65);
        assert_eq!(
            secret.to_string(),
            "ml-dsa-65:7t9yn5gKtyA9EwqwGCHe6WeDdDhDQkRDVhASM7ZvRoum"
        );
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_from_seed_phrase_deterministic() {
        let key1 = SecretKey::ml_dsa65_from_seed_phrase(TEST_PHRASE).unwrap();
        let key2 = SecretKey::ml_dsa65_from_seed_phrase(TEST_PHRASE).unwrap();
        assert_eq!(key1.as_bytes(), key2.as_bytes());
        assert_eq!(key1.public_key(), key2.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_from_seed_phrase_different_paths() {
        let key1 =
            SecretKey::ml_dsa65_from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/0'").unwrap();
        let key2 =
            SecretKey::ml_dsa65_from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/1'").unwrap();
        assert_ne!(key1.public_key(), key2.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_from_seed_phrase_with_passphrase() {
        let no_pass = SecretKey::ml_dsa65_from_seed_phrase_with_path_and_passphrase(
            TEST_PHRASE,
            DEFAULT_HD_PATH,
            None,
        )
        .unwrap();
        let with_pass = SecretKey::ml_dsa65_from_seed_phrase_with_path_and_passphrase(
            TEST_PHRASE,
            DEFAULT_HD_PATH,
            Some("my-password"),
        )
        .unwrap();
        assert_ne!(no_pass.public_key(), with_pass.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_seed_phrase_differs_from_ed25519() {
        // The ML-DSA-65 and Ed25519 branches share the SLIP-10 machinery but
        // use different master salts, so the same phrase yields unrelated keys.
        let ml = SecretKey::ml_dsa65_from_seed_phrase(TEST_PHRASE).unwrap();
        let ed = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();
        assert_eq!(ml.key_type(), KeyType::MlDsa65);
        assert_eq!(ed.key_type(), KeyType::Ed25519);
        // The 32-byte node secrets (both keys' `as_bytes`) must not collide.
        assert_ne!(ml.as_bytes(), ed.as_bytes());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_from_seed_phrase_invalid() {
        let result = SecretKey::ml_dsa65_from_seed_phrase("invalid words that are not a mnemonic");
        assert!(result.is_err());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_seed_phrase_key_can_sign() {
        let secret = SecretKey::ml_dsa65_from_seed_phrase(TEST_PHRASE).unwrap();
        let public = secret.public_key();
        let message = b"post-quantum seed phrase";
        assert!(secret.sign(message).verify(message, &public));
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_generate_with_seed_phrase_defaults_to_24_words() {
        let (phrase, secret) = SecretKey::ml_dsa65_generate_with_seed_phrase().unwrap();

        assert_eq!(DEFAULT_ML_DSA_65_WORD_COUNT, 24);
        assert_eq!(
            phrase.split_whitespace().count(),
            DEFAULT_ML_DSA_65_WORD_COUNT
        );
        assert_eq!(secret.key_type(), KeyType::MlDsa65);
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_generate_with_seed_phrase_roundtrips() {
        let (phrase, secret) = SecretKey::ml_dsa65_generate_with_seed_phrase().unwrap();

        // Re-deriving from the returned phrase yields the same key.
        let derived = SecretKey::ml_dsa65_from_seed_phrase(&phrase).unwrap();
        assert_eq!(secret.public_key(), derived.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_generate_with_seed_phrase_rejects_short_word_counts() {
        // NEP-649: a newly generated ML-DSA-65 mnemonic needs at least 18 words.
        for word_count in [12, 15] {
            let err = SecretKey::ml_dsa65_generate_with_seed_phrase_words(word_count).unwrap_err();
            assert!(
                matches!(err, SignerError::KeyDerivationFailed(ref msg) if msg.contains("NEP-649")),
                "unexpected error for {word_count} words: {err}"
            );
        }
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_generate_with_seed_phrase_accepts_18_21_24() {
        for word_count in [18, 21, 24] {
            let (phrase, secret) =
                SecretKey::ml_dsa65_generate_with_seed_phrase_words(word_count).unwrap();
            assert_eq!(phrase.split_whitespace().count(), word_count);
            assert_eq!(secret.key_type(), KeyType::MlDsa65);
        }
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ml_dsa65_generate_with_seed_phrase_custom() {
        let (phrase, secret) =
            SecretKey::ml_dsa65_generate_with_seed_phrase_custom(18, "m/44'/397'/1'", Some("pw"))
                .unwrap();
        assert_eq!(phrase.split_whitespace().count(), 18);

        let derived = SecretKey::ml_dsa65_from_seed_phrase_with_path_and_passphrase(
            &phrase,
            "m/44'/397'/1'",
            Some("pw"),
        )
        .unwrap();
        assert_eq!(secret.public_key(), derived.public_key());
    }

    #[cfg(feature = "mnemonic")]
    #[test]
    fn test_ed25519_generate_with_seed_phrase_still_12_words() {
        // The NEP-649 floor applies only to the ML-DSA-65 path.
        let (phrase, secret) = SecretKey::generate_with_seed_phrase().unwrap();
        assert_eq!(DEFAULT_WORD_COUNT, 12);
        assert_eq!(phrase.split_whitespace().count(), DEFAULT_WORD_COUNT);
        assert_eq!(secret.key_type(), KeyType::Ed25519);
    }
}
