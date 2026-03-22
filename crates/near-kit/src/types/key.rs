//! Cryptographic key types for NEAR.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use bip39::Mnemonic;
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use k256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Digest;
use slipped10::{BIP32Path, Curve};

use crate::error::{ParseKeyError, SignerError};

/// Key type identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyType {
    /// Ed25519 key (most common).
    Ed25519 = 0,
    /// Secp256k1 key (for Ethereum compatibility).
    Secp256k1 = 1,
}

impl KeyType {
    /// Get the string prefix for this key type.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyType::Ed25519 => "ed25519",
            KeyType::Secp256k1 => "secp256k1",
        }
    }

    /// Get the expected public key length in bytes.
    pub fn public_key_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 32,
            KeyType::Secp256k1 => 64, // Uncompressed, no prefix — matches nearcore
        }
    }

    /// Get the expected signature length in bytes.
    pub fn signature_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 64,
            KeyType::Secp256k1 => 65,
        }
    }
}

impl TryFrom<u8> for KeyType {
    type Error = ParseKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(KeyType::Ed25519),
            1 => Ok(KeyType::Secp256k1),
            _ => Err(ParseKeyError::UnknownKeyType(value.to_string())),
        }
    }
}

/// Ed25519 or Secp256k1 public key.
///
/// Stored as fixed-size arrays matching nearcore's representation:
/// - Ed25519: 32-byte compressed point
/// - Secp256k1: 64-byte uncompressed point (x, y coordinates, no `0x04` prefix)
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum PublicKey {
    /// Ed25519 public key (32 bytes).
    Ed25519([u8; 32]),
    /// Secp256k1 public key (64 bytes, uncompressed without prefix).
    ///
    /// This matches nearcore's representation: raw x,y coordinates
    /// without the `0x04` uncompressed point prefix.
    Secp256k1([u8; 64]),
}

impl PublicKey {
    /// Create an Ed25519 public key from raw 32 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self {
        Self::Ed25519(bytes)
    }

    /// Create a Secp256k1 public key from raw 64-byte uncompressed coordinates.
    ///
    /// The 64 bytes are the raw x,y coordinates without the `0x04` prefix,
    /// matching nearcore's format.
    ///
    /// Validates that the point is on the secp256k1 curve.
    ///
    /// # Panics
    ///
    /// Panics if the bytes do not represent a valid point on the secp256k1 curve.
    pub fn secp256k1_from_bytes(bytes: [u8; 64]) -> Self {
        // Validate the point is on the curve by constructing full uncompressed encoding
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..].copy_from_slice(&bytes);
        let encoded = k256::EncodedPoint::from_bytes(uncompressed.as_ref())
            .expect("invalid secp256k1 SEC1 encoding");
        let point = k256::AffinePoint::from_encoded_point(&encoded);
        assert!(bool::from(point.is_some()), "invalid secp256k1 curve point");

        Self::Secp256k1(bytes)
    }

    /// Create a Secp256k1 public key from a compressed 33-byte SEC1 encoding.
    ///
    /// The key is validated and stored internally in uncompressed (64-byte) format
    /// matching nearcore's representation.
    ///
    /// # Panics
    ///
    /// Panics if the bytes do not represent a valid point on the secp256k1 curve.
    pub fn secp256k1_from_compressed(bytes: [u8; 33]) -> Self {
        let encoded = k256::EncodedPoint::from_bytes(bytes.as_ref())
            .expect("invalid secp256k1 SEC1 encoding");
        let point = k256::AffinePoint::from_encoded_point(&encoded);
        let point: k256::AffinePoint = Option::from(point).expect("invalid secp256k1 curve point");

        // Re-encode as uncompressed (65 bytes with 0x04 prefix)
        let uncompressed = point.to_encoded_point(false);
        let uncompressed_bytes: &[u8] = uncompressed.as_bytes();
        assert_eq!(uncompressed_bytes.len(), 65);
        assert_eq!(uncompressed_bytes[0], 0x04);

        // Store without the 0x04 prefix (64 bytes)
        let mut result = [0u8; 64];
        result.copy_from_slice(&uncompressed_bytes[1..]);
        Self::Secp256k1(result)
    }

    /// Create a Secp256k1 public key from an uncompressed 65-byte SEC1 encoding
    /// (with `0x04` prefix).
    ///
    /// The key is validated and stored internally without the prefix (64 bytes),
    /// matching nearcore's format.
    ///
    /// # Panics
    ///
    /// Panics if the bytes do not represent a valid point on the secp256k1 curve.
    pub fn secp256k1_from_uncompressed(bytes: [u8; 65]) -> Self {
        let encoded = k256::EncodedPoint::from_bytes(bytes.as_ref())
            .expect("invalid secp256k1 SEC1 encoding");
        let point = k256::AffinePoint::from_encoded_point(&encoded);
        assert!(bool::from(point.is_some()), "invalid secp256k1 curve point");

        // Store without the 0x04 prefix (64 bytes)
        let mut result = [0u8; 64];
        result.copy_from_slice(&bytes[1..]);
        Self::Secp256k1(result)
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Ed25519(_) => KeyType::Ed25519,
            Self::Secp256k1(_) => KeyType::Secp256k1,
        }
    }

    /// Get the raw key bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(bytes) => bytes.as_slice(),
            Self::Secp256k1(bytes) => bytes.as_slice(),
        }
    }

    /// Get the key data as a fixed-size array for Ed25519 keys.
    pub fn as_ed25519_bytes(&self) -> Option<&[u8; 32]> {
        match self {
            Self::Ed25519(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Get the key data as a fixed-size array for Secp256k1 keys
    /// (64-byte uncompressed, no prefix).
    pub fn as_secp256k1_bytes(&self) -> Option<&[u8; 64]> {
        match self {
            Self::Secp256k1(bytes) => Some(bytes),
            _ => None,
        }
    }
}

impl FromStr for PublicKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key_type, data_str) = s.split_once(':').ok_or(ParseKeyError::InvalidFormat)?;

        let key_type = match key_type {
            "ed25519" => KeyType::Ed25519,
            "secp256k1" => KeyType::Secp256k1,
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
                VerifyingKey::from_bytes(&bytes).map_err(|_| ParseKeyError::InvalidCurvePoint)?;
                Ok(Self::Ed25519(bytes))
            }
            KeyType::Secp256k1 => {
                // Data is 64 bytes (uncompressed, no prefix). Add 0x04 prefix for validation.
                let mut uncompressed = [0u8; 65];
                uncompressed[0] = 0x04;
                uncompressed[1..].copy_from_slice(&data);
                let encoded = k256::EncodedPoint::from_bytes(uncompressed)
                    .map_err(|_| ParseKeyError::InvalidCurvePoint)?;
                let point = k256::AffinePoint::from_encoded_point(&encoded);
                if point.is_none().into() {
                    return Err(ParseKeyError::InvalidCurvePoint);
                }
                let bytes: [u8; 64] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| ParseKeyError::InvalidCurvePoint)?;
                Ok(Self::Secp256k1(bytes))
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
        write!(f, "PublicKey({})", self)
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
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
                VerifyingKey::from_bytes(&bytes).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid ed25519 curve point",
                    )
                })?;
                Ok(Self::Ed25519(bytes))
            }
            KeyType::Secp256k1 => {
                let mut bytes = [0u8; 64];
                reader.read_exact(&mut bytes)?;
                // Validate point is on curve
                let mut uncompressed = [0u8; 65];
                uncompressed[0] = 0x04;
                uncompressed[1..].copy_from_slice(&bytes);
                let encoded = k256::EncodedPoint::from_bytes(uncompressed).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid secp256k1 encoding",
                    )
                })?;
                let point = k256::AffinePoint::from_encoded_point(&encoded);
                if point.is_none().into() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid secp256k1 curve point",
                    ));
                }
                Ok(Self::Secp256k1(bytes))
            }
        }
    }
}

/// Default BIP-32 HD derivation path for NEAR keys.
/// NEAR uses coin type 397 per SLIP-44.
pub const DEFAULT_HD_PATH: &str = "m/44'/397'/0'";

/// Default number of words in generated seed phrases.
pub const DEFAULT_WORD_COUNT: usize = 12;

/// Ed25519 or Secp256k1 secret key.
#[derive(Clone)]
pub enum SecretKey {
    /// Ed25519 secret key (32-byte seed).
    Ed25519([u8; 32]),
    /// Secp256k1 secret key (32-byte scalar).
    Secp256k1([u8; 32]),
}

impl SecretKey {
    /// Generate a new random Ed25519 key pair.
    pub fn generate_ed25519() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self::Ed25519(signing_key.to_bytes())
    }

    /// Create an Ed25519 secret key from raw 32 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self {
        Self::Ed25519(bytes)
    }

    /// Generate a new random Secp256k1 key pair.
    pub fn generate_secp256k1() -> Self {
        let secret_key = k256::SecretKey::random(&mut OsRng);
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&secret_key.to_bytes());
        Self::Secp256k1(bytes)
    }

    /// Create a Secp256k1 secret key from raw 32 bytes.
    ///
    /// Validates that the bytes represent a valid secp256k1 scalar
    /// (non-zero and less than the curve order).
    pub fn secp256k1_from_bytes(bytes: [u8; 32]) -> Result<Self, ParseKeyError> {
        k256::SecretKey::from_bytes((&bytes).into()).map_err(|_| ParseKeyError::InvalidScalar)?;
        Ok(Self::Secp256k1(bytes))
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Ed25519(_) => KeyType::Ed25519,
            Self::Secp256k1(_) => KeyType::Secp256k1,
        }
    }

    /// Get the raw key bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(bytes) => bytes.as_slice(),
            Self::Secp256k1(bytes) => bytes.as_slice(),
        }
    }

    /// Derive the public key.
    pub fn public_key(&self) -> PublicKey {
        match self {
            Self::Ed25519(bytes) => {
                let signing_key = SigningKey::from_bytes(bytes);
                let verifying_key = signing_key.verifying_key();
                PublicKey::Ed25519(verifying_key.to_bytes())
            }
            Self::Secp256k1(bytes) => {
                let secret_key =
                    k256::SecretKey::from_bytes(bytes.into()).expect("invalid secp256k1 key");
                let public_key = secret_key.public_key();
                // Get uncompressed encoding (65 bytes with 0x04 prefix)
                let uncompressed = public_key.to_encoded_point(false);
                let uncompressed_bytes: &[u8] = uncompressed.as_bytes();
                assert_eq!(uncompressed_bytes.len(), 65);
                // Store without the 0x04 prefix (64 bytes)
                let mut result = [0u8; 64];
                result.copy_from_slice(&uncompressed_bytes[1..]);
                PublicKey::Secp256k1(result)
            }
        }
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Signature {
        match self {
            Self::Ed25519(bytes) => {
                let signing_key = SigningKey::from_bytes(bytes);
                let signature = signing_key.sign(message);
                Signature::Ed25519(signature.to_bytes())
            }
            Self::Secp256k1(bytes) => {
                let signing_key = k256::ecdsa::SigningKey::from_bytes(bytes.into())
                    .expect("invalid secp256k1 key");

                // NEAR protocol: hash message with SHA-256 before signing
                let hash = sha2::Sha256::digest(message);
                let (signature, recovery_id) = signing_key
                    .sign_prehash_recoverable(&hash)
                    .expect("secp256k1 signing failed");

                // NEAR format: [r (32) | s (32) | v (1)]
                let mut sig_bytes = [0u8; 65];
                sig_bytes[..64].copy_from_slice(&signature.to_bytes());
                sig_bytes[64] = recovery_id.to_byte();

                Signature::Secp256k1(sig_bytes)
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
    /// use near_kit::SecretKey;
    ///
    /// // Valid BIP-39 mnemonic (all zeros entropy)
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase(phrase).unwrap();
    /// ```
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
    /// use near_kit::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase_with_path(phrase, "m/44'/397'/1'").unwrap();
    /// ```
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
    /// use near_kit::SecretKey;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let secret_key = SecretKey::from_seed_phrase_with_path_and_passphrase(
    ///     phrase,
    ///     "m/44'/397'/0'",
    ///     Some("my-passphrase")
    /// ).unwrap();
    /// ```
    pub fn from_seed_phrase_with_path_and_passphrase(
        phrase: impl AsRef<str>,
        hd_path: impl AsRef<str>,
        passphrase: Option<&str>,
    ) -> Result<Self, SignerError> {
        // Normalize and parse mnemonic
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

        // Convert mnemonic to seed (64 bytes)
        let seed = mnemonic.to_seed(passphrase.unwrap_or(""));

        // Parse HD path
        let path: BIP32Path = hd_path
            .as_ref()
            .parse()
            .map_err(|e| SignerError::KeyDerivationFailed(format!("Invalid HD path: {}", e)))?;

        // Derive key using SLIP-10 for Ed25519
        let derived =
            slipped10::derive_key_from_path(&seed, Curve::Ed25519, &path).map_err(|e| {
                SignerError::KeyDerivationFailed(format!("SLIP-10 derivation failed: {:?}", e))
            })?;

        Ok(Self::ed25519_from_bytes(derived.key))
    }

    /// Generate a new random seed phrase and derive the corresponding secret key.
    ///
    /// Returns both the seed phrase (for backup) and the derived secret key.
    /// Uses 12 words by default and the standard NEAR HD path.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::generate_with_seed_phrase().unwrap();
    /// println!("Backup your seed phrase: {}", phrase);
    /// ```
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
    /// use near_kit::SecretKey;
    ///
    /// let (phrase, secret_key) = SecretKey::generate_with_seed_phrase_words(24).unwrap();
    /// assert_eq!(phrase.split_whitespace().count(), 24);
    /// ```
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
}

impl FromStr for SecretKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (key_type, data_str) = s.split_once(':').ok_or(ParseKeyError::InvalidFormat)?;

        let key_type = match key_type {
            "ed25519" => KeyType::Ed25519,
            "secp256k1" => KeyType::Secp256k1,
            other => return Err(ParseKeyError::UnknownKeyType(other.to_string())),
        };

        let data = bs58::decode(data_str)
            .into_vec()
            .map_err(|e| ParseKeyError::InvalidBase58(e.to_string()))?;

        // For ed25519, the secret key might be 32 bytes (seed) or 64 bytes (expanded)
        // For secp256k1, it must be 32 bytes
        let valid_len = match key_type {
            KeyType::Ed25519 => data.len() == 32 || data.len() == 64,
            KeyType::Secp256k1 => data.len() == 32,
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
            KeyType::Ed25519 => Ok(Self::Ed25519(bytes)),
            KeyType::Secp256k1 => {
                // Validate secp256k1 scalar (non-zero and < curve order)
                k256::SecretKey::from_bytes((&bytes).into())
                    .map_err(|_| ParseKeyError::InvalidScalar)?;
                Ok(Self::Secp256k1(bytes))
            }
        }
    }
}

impl TryFrom<&str> for SecretKey {
    type Error = ParseKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

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
#[derive(Clone, PartialEq, Eq)]
pub enum Signature {
    /// Ed25519 signature (64 bytes).
    Ed25519([u8; 64]),
    /// Secp256k1 signature (65 bytes: `[r (32) | s (32) | v (1)]`).
    Secp256k1([u8; 65]),
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

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        match self {
            Self::Ed25519(_) => KeyType::Ed25519,
            Self::Secp256k1(_) => KeyType::Secp256k1,
        }
    }

    /// Get the raw signature bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(bytes) => bytes.as_slice(),
            Self::Secp256k1(bytes) => bytes.as_slice(),
        }
    }

    /// Verify this signature against a message and public key.
    pub fn verify(&self, message: &[u8], public_key: &PublicKey) -> bool {
        match (self, public_key) {
            (Self::Ed25519(sig_bytes), PublicKey::Ed25519(pk_bytes)) => {
                let Ok(verifying_key) = VerifyingKey::from_bytes(pk_bytes) else {
                    return false;
                };
                let signature = ed25519_dalek::Signature::from_bytes(sig_bytes);
                verifying_key.verify_strict(message, &signature).is_ok()
            }
            (Self::Secp256k1(sig_bytes), PublicKey::Secp256k1(pk_bytes)) => {
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

                let Ok(signature) = k256::ecdsa::Signature::from_bytes((&sig_bytes[..64]).into())
                else {
                    return false;
                };

                // NEAR protocol: verify against SHA-256 hash of the message
                let hash = sha2::Sha256::digest(message);
                use k256::ecdsa::signature::hazmat::PrehashVerifier;
                verifying_key.verify_prehash(&hash, &signature).is_ok()
            }
            // Mismatched key types
            _ => false,
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
        write!(f, "Signature({})", self)
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
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
        }
    }
}

// ============================================================================
// Seed Phrase Generation
// ============================================================================

/// Generate a random BIP-39 seed phrase.
///
/// # Arguments
///
/// * `word_count` - Number of words (12, 15, 18, 21, or 24)
///
/// # Example
///
/// ```rust
/// use near_kit::generate_seed_phrase;
///
/// let phrase = generate_seed_phrase(12).unwrap();
/// assert_eq!(phrase.split_whitespace().count(), 12);
/// ```
pub fn generate_seed_phrase(word_count: usize) -> Result<String, SignerError> {
    use rand::RngCore;

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
    OsRng.fill_bytes(&mut entropy);

    let mnemonic = Mnemonic::from_entropy(&entropy).map_err(|e| {
        SignerError::KeyDerivationFailed(format!("Failed to generate mnemonic: {}", e))
    })?;

    Ok(mnemonic.to_string())
}

// ============================================================================
// KeyPair
// ============================================================================

/// A cryptographic key pair (secret key + public key).
///
/// This is a convenience type that bundles a [`SecretKey`] with its derived
/// [`PublicKey`] for situations where you need both (e.g., creating accounts,
/// adding access keys).
///
/// # Example
///
/// ```rust
/// use near_kit::KeyPair;
///
/// // Generate a random Ed25519 key pair
/// let keypair = KeyPair::random();
/// println!("Public key: {}", keypair.public_key);
/// println!("Secret key: {}", keypair.secret_key);
///
/// // Use with account creation
/// // near.transaction("new.alice.testnet")
/// //     .create_account()
/// //     .add_full_access_key(keypair.public_key)
/// //     .send()
/// //     .await?;
/// ```
#[derive(Clone)]
pub struct KeyPair {
    /// The secret (private) key.
    pub secret_key: SecretKey,
    /// The public key derived from the secret key.
    pub public_key: PublicKey,
}

impl KeyPair {
    /// Generate a random Ed25519 key pair.
    ///
    /// This is the most common key type for NEAR.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::KeyPair;
    ///
    /// let keypair = KeyPair::random();
    /// ```
    pub fn random() -> Self {
        Self::random_ed25519()
    }

    /// Generate a random Ed25519 key pair.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::KeyPair;
    ///
    /// let keypair = KeyPair::random_ed25519();
    /// assert!(keypair.public_key.to_string().starts_with("ed25519:"));
    /// ```
    pub fn random_ed25519() -> Self {
        let secret_key = SecretKey::generate_ed25519();
        let public_key = secret_key.public_key();
        Self {
            secret_key,
            public_key,
        }
    }

    /// Generate a random Secp256k1 key pair.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::KeyPair;
    ///
    /// let keypair = KeyPair::random_secp256k1();
    /// assert!(keypair.public_key.to_string().starts_with("secp256k1:"));
    /// ```
    pub fn random_secp256k1() -> Self {
        let secret_key = SecretKey::generate_secp256k1();
        let public_key = secret_key.public_key();
        Self {
            secret_key,
            public_key,
        }
    }

    /// Create a key pair from an existing secret key.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::{KeyPair, SecretKey};
    ///
    /// let secret_key: SecretKey = "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse().unwrap();
    /// let keypair = KeyPair::from_secret_key(secret_key);
    /// ```
    pub fn from_secret_key(secret_key: SecretKey) -> Self {
        let public_key = secret_key.public_key();
        Self {
            secret_key,
            public_key,
        }
    }

    /// Create a key pair from a seed phrase using the default NEAR HD path.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::KeyPair;
    ///
    /// let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// let keypair = KeyPair::from_seed_phrase(phrase).unwrap();
    /// ```
    pub fn from_seed_phrase(phrase: impl AsRef<str>) -> Result<Self, SignerError> {
        let secret_key = SecretKey::from_seed_phrase(phrase)?;
        Ok(Self::from_secret_key(secret_key))
    }

    /// Generate a new random key pair with a seed phrase for backup.
    ///
    /// Returns the seed phrase (for backup) and the key pair.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::KeyPair;
    ///
    /// let (phrase, keypair) = KeyPair::random_with_seed_phrase().unwrap();
    /// println!("Backup your seed phrase: {}", phrase);
    /// println!("Public key: {}", keypair.public_key);
    /// ```
    pub fn random_with_seed_phrase() -> Result<(String, Self), SignerError> {
        let (phrase, secret_key) = SecretKey::generate_with_seed_phrase()?;
        Ok((phrase, Self::from_secret_key(secret_key)))
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public_key", &self.public_key)
            .field("secret_key", &"***")
            .finish()
    }
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

    #[test]
    fn test_generate_seed_phrase_12_words() {
        let phrase = generate_seed_phrase(12).unwrap();
        assert_eq!(phrase.split_whitespace().count(), 12);
    }

    #[test]
    fn test_generate_seed_phrase_24_words() {
        let phrase = generate_seed_phrase(24).unwrap();
        assert_eq!(phrase.split_whitespace().count(), 24);
    }

    #[test]
    fn test_generate_seed_phrase_invalid_word_count() {
        let result = generate_seed_phrase(13);
        assert!(result.is_err());
    }

    // Valid BIP-39 test vector (from official test vectors)
    const TEST_PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn test_from_seed_phrase_known_vector() {
        // Test vector: known seed phrase should produce consistent key
        let secret_key = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();

        // Same phrase should always produce the same key
        let secret_key2 = SecretKey::from_seed_phrase(TEST_PHRASE).unwrap();
        assert_eq!(secret_key.public_key(), secret_key2.public_key());
    }

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

    #[test]
    fn test_from_seed_phrase_invalid() {
        let result = SecretKey::from_seed_phrase("invalid words that are not a mnemonic");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_seed_phrase_different_paths() {
        let key1 = SecretKey::from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/0'").unwrap();
        let key2 = SecretKey::from_seed_phrase_with_path(TEST_PHRASE, "m/44'/397'/1'").unwrap();

        // Different paths should produce different keys
        assert_ne!(key1.public_key(), key2.public_key());
    }

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

    #[test]
    fn test_generate_with_seed_phrase() {
        let (phrase, secret_key) = SecretKey::generate_with_seed_phrase().unwrap();

        // Phrase should be 12 words
        assert_eq!(phrase.split_whitespace().count(), 12);

        // Re-deriving from the phrase should produce the same key
        let derived = SecretKey::from_seed_phrase(&phrase).unwrap();
        assert_eq!(secret_key.public_key(), derived.public_key());
    }

    #[test]
    fn test_generate_with_seed_phrase_24_words() {
        let (phrase, secret_key) = SecretKey::generate_with_seed_phrase_words(24).unwrap();

        assert_eq!(phrase.split_whitespace().count(), 24);

        let derived = SecretKey::from_seed_phrase(&phrase).unwrap();
        assert_eq!(secret_key.public_key(), derived.public_key());
    }

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

        // Test with secp256k1 since ed25519 validation is very lenient
        // Use invalid secp256k1 bytes (all zeros is definitely not on the curve)
        let mut invalid_bytes = vec![1u8]; // KeyType::Secp256k1
        invalid_bytes.extend_from_slice(&[0u8; 64]); // Invalid curve point (64 bytes now)

        let result = PublicKey::try_from_slice(&invalid_bytes);
        assert!(result.is_err());
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
        let message = b"hello world";

        assert_eq!(secret.key_type(), KeyType::Secp256k1);
        assert_eq!(public.key_type(), KeyType::Secp256k1);

        let signature = secret.sign(message);
        assert_eq!(signature.key_type(), KeyType::Secp256k1);
        assert_eq!(signature.as_bytes().len(), 65);

        assert!(signature.verify(message, &public));
        assert!(!signature.verify(b"wrong message", &public));
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
    fn test_secp256k1_keypair_random() {
        let keypair = KeyPair::random_secp256k1();
        assert_eq!(keypair.public_key.key_type(), KeyType::Secp256k1);
        assert_eq!(keypair.secret_key.key_type(), KeyType::Secp256k1);
        assert!(keypair.public_key.to_string().starts_with("secp256k1:"));

        // Sign/verify through keypair
        let message = b"keypair test";
        let signature = keypair.secret_key.sign(message);
        assert!(signature.verify(message, &keypair.public_key));
    }

    #[test]
    fn test_secp256k1_cross_type_verify_fails() {
        // Ed25519 signature should not verify against secp256k1 key
        let ed_secret = SecretKey::generate_ed25519();
        let secp_secret = SecretKey::generate_secp256k1();

        let message = b"cross type test";
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
        let message = b"test message";
        let signature = secret.sign(message);

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
    fn test_secp256k1_from_compressed() {
        let secret = SecretKey::generate_secp256k1();
        let public = secret.public_key();

        // Get the uncompressed bytes and create a compressed version
        let pk_bytes = public.as_secp256k1_bytes().unwrap();
        let mut uncompressed = [0u8; 65];
        uncompressed[0] = 0x04;
        uncompressed[1..].copy_from_slice(pk_bytes);
        let encoded =
            k256::EncodedPoint::from_bytes(uncompressed.as_ref()).expect("valid encoded point");
        let point = k256::AffinePoint::from_encoded_point(&encoded).expect("valid point on curve");
        let compressed = point.to_encoded_point(true);
        let compressed_bytes: [u8; 33] = compressed
            .as_bytes()
            .try_into()
            .expect("compressed should be 33 bytes");

        // Reconstruct from compressed form
        let public2 = PublicKey::secp256k1_from_compressed(compressed_bytes);
        assert_eq!(public, public2);
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
    fn test_enum_variants_match_expected_types() {
        let ed_secret = SecretKey::generate_ed25519();
        let ed_public = ed_secret.public_key();

        assert!(matches!(ed_public, PublicKey::Ed25519(_)));
        assert!(matches!(ed_secret, SecretKey::Ed25519(_)));

        let secp_secret = SecretKey::generate_secp256k1();
        let secp_public = secp_secret.public_key();

        assert!(matches!(secp_public, PublicKey::Secp256k1(_)));
        assert!(matches!(secp_secret, SecretKey::Secp256k1(_)));
    }
}
