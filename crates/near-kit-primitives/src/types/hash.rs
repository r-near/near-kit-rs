//! Cryptographic hash type.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use sha2::{Digest, Sha256};

use crate::error::ParseHashError;

/// A 32-byte SHA-256 hash used for block hashes, transaction hashes, etc.
///
/// # near-sdk interop
///
/// near-sdk's `CryptoHash` is `pub type CryptoHash = [u8; 32]`, so converting
/// in either direction is just `.into()`, zero-cost and with no near-sdk
/// dependency:
///
/// ```rust
/// # let sdk_hash: [u8; 32] = [7; 32];
/// let kit: near_kit_primitives::CryptoHash = sdk_hash.into(); // [u8; 32] -> CryptoHash
/// let sdk: [u8; 32] = kit.into();                  // CryptoHash -> [u8; 32]
/// # assert_eq!(sdk, sdk_hash);
/// ```
///
/// near-sdk's JSON-facing `Base58CryptoHash` converts through `[u8; 32]` the
/// same way.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    SerializeDisplay,
    DeserializeFromStr,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct CryptoHash([u8; 32]);

impl CryptoHash {
    /// The zero hash (32 zero bytes).
    pub const ZERO: Self = Self([0; 32]);

    /// Hash the given data with SHA-256.
    pub fn hash(data: &[u8]) -> Self {
        Self(Sha256::digest(data).into())
    }

    /// Create from raw 32 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the raw 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to a `Vec<u8>`.
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
        Self::try_from(bytes.as_slice())
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
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| ParseHashError::InvalidLength(bytes.len()))
    }
}

impl From<[u8; 32]> for CryptoHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<CryptoHash> for [u8; 32] {
    fn from(hash: CryptoHash) -> Self {
        hash.0
    }
}

impl From<&CryptoHash> for [u8; 32] {
    fn from(hash: &CryptoHash) -> Self {
        hash.0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_parse_roundtrip() {
        let hash = CryptoHash::hash(b"test data");
        let parsed: CryptoHash = hash.to_string().parse().unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn test_zero_and_default() {
        assert!(CryptoHash::ZERO.is_zero());
        assert!(!CryptoHash::hash(b"x").is_zero());
        assert_eq!(CryptoHash::default(), CryptoHash::ZERO);
    }

    #[test]
    fn test_debug_format() {
        let debug = format!("{:?}", CryptoHash::ZERO);
        assert!(debug.starts_with("CryptoHash("));
        assert!(debug.contains("1111111111")); // Zero hash in base58
    }

    #[test]
    fn test_parse_invalid_base58() {
        let result: Result<CryptoHash, _> = "invalid!@#$%base58".parse();
        assert!(matches!(result, Err(ParseHashError::InvalidBase58(_))));
    }

    #[test]
    fn test_parse_wrong_length() {
        // Valid base58 but too short.
        let result: Result<CryptoHash, _> = "3xRDxw".parse();
        assert!(matches!(result, Err(ParseHashError::InvalidLength(_))));
        let result = CryptoHash::try_from([3u8; 16].as_slice());
        assert!(matches!(result, Err(ParseHashError::InvalidLength(16))));
    }

    #[test]
    fn test_serde_roundtrip() {
        let hash = CryptoHash::hash(b"serde test");
        let json = serde_json::to_string(&hash).unwrap();
        let parsed: CryptoHash = serde_json::from_str(&json).unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn test_borsh_roundtrip() {
        let hash = CryptoHash::hash(b"borsh test");
        let bytes = borsh::to_vec(&hash).unwrap();
        assert_eq!(bytes, hash.as_bytes());
        let parsed: CryptoHash = borsh::from_slice(&bytes).unwrap();
        assert_eq!(hash, parsed);
    }
}
