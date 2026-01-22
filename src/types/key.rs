//! Cryptographic key types for NEAR.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ParseKeyError;

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

    /// Get the expected key length in bytes.
    pub fn key_len(&self) -> usize {
        match self {
            KeyType::Ed25519 => 32,
            KeyType::Secp256k1 => 33, // Compressed
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
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PublicKey {
    key_type: KeyType,
    data: Vec<u8>,
}

impl PublicKey {
    /// Create an Ed25519 public key from raw 32 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            key_type: KeyType::Ed25519,
            data: bytes.to_vec(),
        }
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        self.key_type
    }

    /// Get the raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Get the key data as a fixed-size array for Ed25519 keys.
    pub fn as_ed25519_bytes(&self) -> Option<&[u8; 32]> {
        if self.key_type == KeyType::Ed25519 && self.data.len() == 32 {
            Some(self.data.as_slice().try_into().unwrap())
        } else {
            None
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

        if data.len() != key_type.key_len() {
            return Err(ParseKeyError::InvalidLength {
                expected: key_type.key_len(),
                actual: data.len(),
            });
        }

        Ok(Self { key_type, data })
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
            self.key_type.as_str(),
            bs58::encode(&self.data).into_string()
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
        borsh::BorshSerialize::serialize(&(self.key_type as u8), writer)?;
        writer.write_all(&self.data)?;
        Ok(())
    }
}

impl BorshDeserialize for PublicKey {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let key_type_byte = u8::deserialize_reader(reader)?;
        let key_type = KeyType::try_from(key_type_byte)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut data = vec![0u8; key_type.key_len()];
        reader.read_exact(&mut data)?;

        Ok(Self { key_type, data })
    }
}

/// Ed25519 or Secp256k1 secret key.
#[derive(Clone)]
pub struct SecretKey {
    key_type: KeyType,
    data: Vec<u8>,
}

impl SecretKey {
    /// Generate a new random Ed25519 key pair.
    pub fn generate_ed25519() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self {
            key_type: KeyType::Ed25519,
            data: signing_key.to_bytes().to_vec(),
        }
    }

    /// Create an Ed25519 secret key from raw 32 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            key_type: KeyType::Ed25519,
            data: bytes.to_vec(),
        }
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        self.key_type
    }

    /// Get the raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Derive the public key.
    pub fn public_key(&self) -> PublicKey {
        match self.key_type {
            KeyType::Ed25519 => {
                let bytes: [u8; 32] = self
                    .data
                    .as_slice()
                    .try_into()
                    .expect("invalid ed25519 key");
                let signing_key = SigningKey::from_bytes(&bytes);
                let verifying_key = signing_key.verifying_key();
                PublicKey::ed25519_from_bytes(verifying_key.to_bytes())
            }
            KeyType::Secp256k1 => {
                unimplemented!("secp256k1 not yet supported")
            }
        }
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Signature {
        match self.key_type {
            KeyType::Ed25519 => {
                let bytes: [u8; 32] = self
                    .data
                    .as_slice()
                    .try_into()
                    .expect("invalid ed25519 key");
                let signing_key = SigningKey::from_bytes(&bytes);
                let signature = signing_key.sign(message);
                Signature {
                    key_type: KeyType::Ed25519,
                    data: signature.to_bytes().to_vec(),
                }
            }
            KeyType::Secp256k1 => {
                unimplemented!("secp256k1 not yet supported")
            }
        }
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
        if key_type == KeyType::Ed25519 && data.len() != 32 && data.len() != 64 {
            return Err(ParseKeyError::InvalidLength {
                expected: 32,
                actual: data.len(),
            });
        } else if key_type == KeyType::Secp256k1 && data.len() != 32 {
            return Err(ParseKeyError::InvalidLength {
                expected: 32,
                actual: data.len(),
            });
        }

        // Take first 32 bytes if 64-byte expanded key
        let data = if data.len() == 64 {
            data[..32].to_vec()
        } else {
            data
        };

        Ok(Self { key_type, data })
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
            self.key_type.as_str(),
            bs58::encode(&self.data).into_string()
        )
    }
}

impl Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({}:***)", self.key_type.as_str())
    }
}

/// Cryptographic signature.
#[derive(Clone, PartialEq, Eq)]
pub struct Signature {
    key_type: KeyType,
    data: Vec<u8>,
}

impl Signature {
    /// Create an Ed25519 signature from raw 64 bytes.
    pub fn ed25519_from_bytes(bytes: [u8; 64]) -> Self {
        Self {
            key_type: KeyType::Ed25519,
            data: bytes.to_vec(),
        }
    }

    /// Get the key type.
    pub fn key_type(&self) -> KeyType {
        self.key_type
    }

    /// Get the raw signature bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Verify this signature against a message and public key.
    pub fn verify(&self, message: &[u8], public_key: &PublicKey) -> bool {
        if self.key_type != public_key.key_type() {
            return false;
        }

        match self.key_type {
            KeyType::Ed25519 => {
                let Some(pk_bytes) = public_key.as_ed25519_bytes() else {
                    return false;
                };
                let Ok(verifying_key) = VerifyingKey::from_bytes(pk_bytes) else {
                    return false;
                };
                let sig_bytes: [u8; 64] = match self.data.as_slice().try_into() {
                    Ok(b) => b,
                    Err(_) => return false,
                };
                let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
                verifying_key.verify_strict(message, &signature).is_ok()
            }
            KeyType::Secp256k1 => {
                unimplemented!("secp256k1 not yet supported")
            }
        }
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}",
            self.key_type.as_str(),
            bs58::encode(&self.data).into_string()
        )
    }
}

impl Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", self)
    }
}

impl BorshSerialize for Signature {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&(self.key_type as u8), writer)?;
        writer.write_all(&self.data)?;
        Ok(())
    }
}

impl BorshDeserialize for Signature {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let key_type_byte = u8::deserialize_reader(reader)?;
        let key_type = KeyType::try_from(key_type_byte)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut data = vec![0u8; key_type.signature_len()];
        reader.read_exact(&mut data)?;

        Ok(Self { key_type, data })
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
}
