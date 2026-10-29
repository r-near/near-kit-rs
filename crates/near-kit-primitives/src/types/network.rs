//! Chain identification for NEAR Protocol.

use std::fmt;

/// Identifies the NEAR chain the client is connected to.
///
/// Any string is a chain ID. Use the named constructors for well-known chains, or [`ChainId::new`]
/// for custom ones.
///
/// # Examples
///
/// ```
/// use near_kit_primitives::protocol::ChainId;
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

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ChainId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ChainId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id() {
        assert_eq!(ChainId::default(), ChainId::mainnet());
        assert_eq!(ChainId::mainnet().to_string(), "mainnet");
        assert!(ChainId::mainnet().is_mainnet() && !ChainId::mainnet().is_testnet());
        assert!(ChainId::testnet().is_testnet() && !ChainId::testnet().is_mainnet());

        let custom = ChainId::from("my-custom-network");
        assert_eq!(custom, ChainId::from(String::from("my-custom-network")));
        assert_eq!(custom.as_str(), "my-custom-network");
        assert!(!custom.is_mainnet() && !custom.is_testnet());
    }
}
