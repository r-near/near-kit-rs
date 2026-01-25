//! Network identification for NEAR Protocol.

use std::fmt;

/// The NEAR network the client is connected to.
///
/// This is used to resolve network-specific addresses for known tokens
/// and other network-aware operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Network {
    /// NEAR mainnet (production network).
    #[default]
    Mainnet,
    /// NEAR testnet (testing network).
    Testnet,
    /// Local sandbox network for development.
    Sandbox,
    /// Custom network with unknown token mappings.
    Custom,
}

impl Network {
    /// Returns true if this is mainnet.
    pub fn is_mainnet(&self) -> bool {
        matches!(self, Network::Mainnet)
    }

    /// Returns true if this is testnet.
    pub fn is_testnet(&self) -> bool {
        matches!(self, Network::Testnet)
    }

    /// Returns true if this is a sandbox network.
    pub fn is_sandbox(&self) -> bool {
        matches!(self, Network::Sandbox)
    }

    /// Returns the network identifier string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Sandbox => "sandbox",
            Network::Custom => "custom",
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_display() {
        assert_eq!(Network::Mainnet.to_string(), "mainnet");
        assert_eq!(Network::Testnet.to_string(), "testnet");
        assert_eq!(Network::Sandbox.to_string(), "sandbox");
        assert_eq!(Network::Custom.to_string(), "custom");
    }

    #[test]
    fn test_network_predicates() {
        assert!(Network::Mainnet.is_mainnet());
        assert!(!Network::Mainnet.is_testnet());

        assert!(Network::Testnet.is_testnet());
        assert!(!Network::Testnet.is_mainnet());

        assert!(Network::Sandbox.is_sandbox());
    }

    #[test]
    fn test_default_is_mainnet() {
        assert_eq!(Network::default(), Network::Mainnet);
    }
}
