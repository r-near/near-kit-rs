//! Known token addresses for common FT contracts.
//!
//! This module provides verified contract addresses for popular tokens,
//! automatically resolving to the correct address based on the network.
//!
//! # Example
//!
//! ```rust,no_run
//! use near_kit::{Near, tokens};
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let near = Near::mainnet().build();
//!
//! // Use known token constants - auto-resolves based on network
//! let usdc = near.ft(tokens::USDC)?;
//! let balance = usdc.balance_of("alice.near").await?;
//!
//! // Still works with raw addresses
//! let custom = near.ft("custom-token.near")?;
//! # Ok(())
//! # }
//! ```

use crate::error::Error;
use crate::types::{AccountId, Network};

/// A known fungible token with verified addresses for different networks.
///
/// Use the predefined constants like [`USDC`], [`USDT`], and [`W_NEAR`]
/// for common tokens.
#[derive(Debug, Clone, Copy)]
pub struct KnownToken {
    /// Human-readable name for error messages.
    pub name: &'static str,
    /// Contract address on mainnet.
    pub mainnet: &'static str,
    /// Contract address on testnet (if available).
    pub testnet: Option<&'static str>,
}

impl KnownToken {
    /// Resolve this token to an [`AccountId`] for the given network.
    ///
    /// # Errors
    ///
    /// Returns an error if the token is not available on the specified network
    /// (e.g., some tokens don't have testnet deployments).
    pub fn resolve(&self, network: Network) -> Result<AccountId, Error> {
        let address = match network {
            Network::Mainnet => self.mainnet,
            Network::Testnet => self.testnet.ok_or_else(|| Error::TokenNotAvailable {
                token: self.name.to_string(),
                network: network.to_string(),
            })?,
            Network::Sandbox | Network::Custom => {
                return Err(Error::TokenNotAvailable {
                    token: self.name.to_string(),
                    network: network.to_string(),
                });
            }
        };
        address.parse().map_err(Into::into)
    }
}

/// Trait for types that can be resolved to a contract [`AccountId`].
///
/// This enables the `ft()` and `nft()` methods to accept both raw addresses
/// and [`KnownToken`] constants, resolving them based on the client's network.
pub trait IntoContractId {
    /// Resolve this to a contract [`AccountId`] for the given network.
    fn into_contract_id(self, network: Network) -> Result<AccountId, Error>;
}

impl IntoContractId for &str {
    fn into_contract_id(self, _network: Network) -> Result<AccountId, Error> {
        self.parse().map_err(Into::into)
    }
}

impl IntoContractId for String {
    fn into_contract_id(self, _network: Network) -> Result<AccountId, Error> {
        self.parse().map_err(Into::into)
    }
}

impl IntoContractId for AccountId {
    fn into_contract_id(self, _network: Network) -> Result<AccountId, Error> {
        Ok(self)
    }
}

impl IntoContractId for &AccountId {
    fn into_contract_id(self, _network: Network) -> Result<AccountId, Error> {
        Ok(self.clone())
    }
}

impl IntoContractId for KnownToken {
    fn into_contract_id(self, network: Network) -> Result<AccountId, Error> {
        self.resolve(network)
    }
}

// =============================================================================
// Stablecoins
// =============================================================================

/// USDC (USD Coin) - Circle's USD stablecoin.
///
/// - Mainnet: `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`
/// - Testnet: `3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af`
pub const USDC: KnownToken = KnownToken {
    name: "USDC",
    mainnet: "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
    testnet: Some("3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af"),
};

/// USDT (Tether USD) - Tether's USD stablecoin.
///
/// - Mainnet: `usdt.tether-token.near`
/// - Testnet: Not available
pub const USDT: KnownToken = KnownToken {
    name: "USDT",
    mainnet: "usdt.tether-token.near",
    testnet: None,
};

// =============================================================================
// Wrapped Tokens
// =============================================================================

/// wNEAR (Wrapped NEAR) - Wrapped version of NEAR for DeFi compatibility.
///
/// - Mainnet: `wrap.near`
/// - Testnet: `wrap.testnet`
pub const W_NEAR: KnownToken = KnownToken {
    name: "wNEAR",
    mainnet: "wrap.near",
    testnet: Some("wrap.testnet"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdc_mainnet() {
        let account = USDC.resolve(Network::Mainnet).unwrap();
        assert_eq!(
            account.as_str(),
            "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"
        );
    }

    #[test]
    fn test_usdc_testnet() {
        let account = USDC.resolve(Network::Testnet).unwrap();
        assert_eq!(
            account.as_str(),
            "3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af"
        );
    }

    #[test]
    fn test_usdt_mainnet() {
        let account = USDT.resolve(Network::Mainnet).unwrap();
        assert_eq!(account.as_str(), "usdt.tether-token.near");
    }

    #[test]
    fn test_usdt_testnet_not_available() {
        let result = USDT.resolve(Network::Testnet);
        assert!(result.is_err());
    }

    #[test]
    fn test_wnear_mainnet() {
        let account = W_NEAR.resolve(Network::Mainnet).unwrap();
        assert_eq!(account.as_str(), "wrap.near");
    }

    #[test]
    fn test_wnear_testnet() {
        let account = W_NEAR.resolve(Network::Testnet).unwrap();
        assert_eq!(account.as_str(), "wrap.testnet");
    }

    #[test]
    fn test_sandbox_not_available() {
        let result = USDC.resolve(Network::Sandbox);
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_not_available() {
        let result = USDC.resolve(Network::Custom);
        assert!(result.is_err());
    }
}
