//! Cryptographic hash type.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::ParseHashError;

/// A 32-byte SHA-256 hash used for block hashes, transaction hashes, etc.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CryptoHash([u8; 32]);

impl CryptoHash {
    /// The zero hash (32 zero bytes).
    pub const ZERO: Self = Self([0; 32]);

    /// Hash the given data with SHA-256.
    pub fn hash(data: &[u8]) -> Self {
        let result = Sha256::digest(data);
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }

    /// Create from raw 32 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the raw 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to a Vec<u8>.
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Check if this is the zero hash.
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }
}

impl FromStr for CryptoHash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|e| ParseHashError::InvalidBase58(e.to_string()))?;

        if bytes.len() != 32 {
            return Err(ParseHashError::InvalidLength(bytes.len()));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl TryFrom<&str> for CryptoHash {
    type Error = ParseHashError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<&[u8]> for CryptoHash {
    type Error = ParseHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 32 {
            return Err(ParseHashError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }
}

impl From<[u8; 32]> for CryptoHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for CryptoHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for CryptoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", bs58::encode(&self.0).into_string())
    }
}

impl Debug for CryptoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CryptoHash({})", self)
    }
}

impl Serialize for CryptoHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CryptoHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl BorshSerialize for CryptoHash {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.0)
    }
}

impl BorshDeserialize for CryptoHash {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash() {
        let hash = CryptoHash::hash(b"hello world");
        assert!(!hash.is_zero());
        assert_eq!(hash.as_bytes().len(), 32);
    }

    #[test]
    fn test_display_parse_roundtrip() {
        let hash = CryptoHash::hash(b"test data");
        let s = hash.to_string();
        let parsed: CryptoHash = s.parse().unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn test_zero() {
        assert!(CryptoHash::ZERO.is_zero());
        assert!(!CryptoHash::hash(b"x").is_zero());
    }
}
