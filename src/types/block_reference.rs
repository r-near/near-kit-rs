//! Block reference types for RPC queries.

use serde::{Deserialize, Serialize};

use super::CryptoHash;

/// Reference to a specific block for RPC queries.
///
/// Every NEAR RPC query operates on state at a specific block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockReference {
    /// Query at latest block with specified finality.
    Finality(Finality),
    /// Query at specific block height.
    Height(u64),
    /// Query at specific block hash.
    Hash(CryptoHash),
}

impl Default for BlockReference {
    fn default() -> Self {
        Self::Finality(Finality::Final)
    }
}

impl BlockReference {
    /// Query at final block.
    pub fn final_() -> Self {
        Self::Finality(Finality::Final)
    }

    /// Query at optimistic (latest) block.
    pub fn optimistic() -> Self {
        Self::Finality(Finality::Optimistic)
    }

    /// Query at near-final block.
    pub fn near_final() -> Self {
        Self::Finality(Finality::NearFinal)
    }

    /// Query at specific height.
    pub fn at_height(height: u64) -> Self {
        Self::Height(height)
    }

    /// Query at specific hash.
    pub fn at_hash(hash: CryptoHash) -> Self {
        Self::Hash(hash)
    }

    /// Convert to JSON for RPC requests.
    pub fn to_rpc_params(&self) -> serde_json::Value {
        match self {
            BlockReference::Finality(f) => {
                serde_json::json!({ "finality": f.as_str() })
            }
            BlockReference::Height(h) => {
                serde_json::json!({ "block_id": *h })
            }
            BlockReference::Hash(h) => {
                serde_json::json!({ "block_id": h.to_string() })
            }
        }
    }
}

impl From<Finality> for BlockReference {
    fn from(f: Finality) -> Self {
        Self::Finality(f)
    }
}

impl From<u64> for BlockReference {
    fn from(height: u64) -> Self {
        Self::Height(height)
    }
}

impl From<CryptoHash> for BlockReference {
    fn from(hash: CryptoHash) -> Self {
        Self::Hash(hash)
    }
}

/// Finality level for queries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Finality {
    /// Latest optimistic block. Fastest, but may be reorged.
    Optimistic,
    /// Doomslug finality. Irreversible unless validator slashed.
    #[serde(rename = "near-final")]
    NearFinal,
    /// Fully finalized. Slowest, 100% guaranteed.
    #[default]
    Final,
}

impl Finality {
    /// Get the string representation for RPC.
    pub fn as_str(&self) -> &'static str {
        match self {
            Finality::Optimistic => "optimistic",
            Finality::NearFinal => "near-final",
            Finality::Final => "final",
        }
    }
}

/// Transaction execution status for send_tx wait_until parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TxExecutionStatus {
    /// Don't wait, return immediately after RPC accepts.
    None,
    /// Wait for inclusion in a block.
    Included,
    /// Wait for execution (optimistic).
    #[default]
    ExecutedOptimistic,
    /// Wait for inclusion in final block.
    IncludedFinal,
    /// Wait for execution in final block.
    Executed,
    /// Wait for full finality.
    Final,
}

impl TxExecutionStatus {
    /// Get the string representation for RPC.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Included => "INCLUDED",
            Self::ExecutedOptimistic => "EXECUTED_OPTIMISTIC",
            Self::IncludedFinal => "INCLUDED_FINAL",
            Self::Executed => "EXECUTED",
            Self::Final => "FINAL",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_reference_rpc_params() {
        let final_ref = BlockReference::final_();
        let params = final_ref.to_rpc_params();
        assert_eq!(params["finality"], "final");

        let height_ref = BlockReference::Height(12345);
        let params = height_ref.to_rpc_params();
        assert_eq!(params["block_id"], 12345);
    }

    #[test]
    fn test_finality_as_str() {
        assert_eq!(Finality::Final.as_str(), "final");
        assert_eq!(Finality::Optimistic.as_str(), "optimistic");
        assert_eq!(Finality::NearFinal.as_str(), "near-final");
    }

    #[test]
    fn test_tx_execution_status_as_str() {
        assert_eq!(TxExecutionStatus::Final.as_str(), "FINAL");
        assert_eq!(
            TxExecutionStatus::ExecutedOptimistic.as_str(),
            "EXECUTED_OPTIMISTIC"
        );
    }
}
