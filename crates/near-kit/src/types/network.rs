//! Chain identification for NEAR Protocol.

use std::fmt;

/// Identifies the NEAR chain the client is connected to.
///
/// This is a string-based newtype that replaces the former `Network` enum,
/// allowing any chain identifier (not just the hardcoded variants).
///
/// Use the named constructors for well-known chains, or [`ChainId::new`]
/// for custom ones.
///
/// # Examples
///
/// ```
/// use near_kit::ChainId;
///
/// let mainnet = ChainId::mainnet();
/// assert!(mainnet.is_mainnet());
/// assert_eq!(mainnet.as_str(), "mainnet");
///
/// let custom = ChainId::new("localnet");
/// assert_eq!(custom.as_str(), "localnet");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChainId(String);

impl ChainId {
    /// Create a chain ID from any string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// NEAR mainnet.
    pub fn mainnet() -> Self {
        Self("mainnet".to_string())
    }

    /// NEAR testnet.
    pub fn testnet() -> Self {
        Self("testnet".to_string())
    }

    /// Returns true if this is mainnet.
    pub fn is_mainnet(&self) -> bool {
        self.0 == "mainnet"
    }

    /// Returns true if this is testnet.
    pub fn is_testnet(&self) -> bool {
        self.0 == "testnet"
    }

    /// Returns the chain identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ChainId {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl AsRef<str> for ChainId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_display() {
        assert_eq!(ChainId::mainnet().to_string(), "mainnet");
        assert_eq!(ChainId::testnet().to_string(), "testnet");
        assert_eq!(ChainId::new("custom").to_string(), "custom");
    }

    #[test]
    fn test_chain_id_predicates() {
        assert!(ChainId::mainnet().is_mainnet());
        assert!(!ChainId::mainnet().is_testnet());

        assert!(ChainId::testnet().is_testnet());
        assert!(!ChainId::testnet().is_mainnet());
    }

    #[test]
    fn test_default_is_mainnet() {
        assert_eq!(ChainId::default(), ChainId::mainnet());
    }

    #[test]
    fn test_as_str() {
        assert_eq!(ChainId::mainnet().as_str(), "mainnet");
        assert_eq!(ChainId::new("localnet").as_str(), "localnet");
    }

    #[test]
    fn test_as_ref_str() {
        let chain_id = ChainId::mainnet();
        let s: &str = chain_id.as_ref();
        assert_eq!(s, "mainnet");
    }

    #[test]
    fn test_custom_chain_id() {
        let chain_id = ChainId::new("my-custom-network");
        assert_eq!(chain_id.as_str(), "my-custom-network");
        assert!(!chain_id.is_mainnet());
        assert!(!chain_id.is_testnet());
    }
}
