//! Block reference types for RPC queries.

use serde::{Deserialize, Serialize};

use super::CryptoHash;

/// Sync checkpoint for block references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncCheckpoint {
    /// Genesis block.
    Genesis,
    /// Earliest available block.
    EarliestAvailable,
}

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
    /// Query at a sync checkpoint (genesis or earliest available).
    SyncCheckpoint(SyncCheckpoint),
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

    /// Query at genesis block.
    pub fn genesis() -> Self {
        Self::SyncCheckpoint(SyncCheckpoint::Genesis)
    }

    /// Query at earliest available block.
    pub fn earliest_available() -> Self {
        Self::SyncCheckpoint(SyncCheckpoint::EarliestAvailable)
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
            BlockReference::SyncCheckpoint(cp) => {
                let cp_str = match cp {
                    SyncCheckpoint::Genesis => "genesis",
                    SyncCheckpoint::EarliestAvailable => "earliest_available",
                };
                serde_json::json!({ "sync_checkpoint": cp_str })
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

    #[test]
    fn test_block_reference_hash_to_rpc_params() {
        let hash = CryptoHash::hash(b"test block");
        let block_ref = BlockReference::at_hash(hash);
        let params = block_ref.to_rpc_params();
        assert_eq!(params["block_id"], hash.to_string());
    }

    #[test]
    fn test_block_reference_constructors() {
        // Test all constructor methods
        let final_ref = BlockReference::final_();
        assert!(matches!(
            final_ref,
            BlockReference::Finality(Finality::Final)
        ));

        let optimistic_ref = BlockReference::optimistic();
        assert!(matches!(
            optimistic_ref,
            BlockReference::Finality(Finality::Optimistic)
        ));

        let near_final_ref = BlockReference::near_final();
        assert!(matches!(
            near_final_ref,
            BlockReference::Finality(Finality::NearFinal)
        ));

        let height_ref = BlockReference::at_height(12345);
        assert!(matches!(height_ref, BlockReference::Height(12345)));

        let hash = CryptoHash::hash(b"test");
        let hash_ref = BlockReference::at_hash(hash);
        assert!(matches!(hash_ref, BlockReference::Hash(_)));
    }

    #[test]
    fn test_block_reference_default() {
        let default = BlockReference::default();
        assert_eq!(default, BlockReference::Finality(Finality::Final));
    }

    #[test]
    fn test_block_reference_from_finality() {
        let block_ref: BlockReference = Finality::Optimistic.into();
        assert_eq!(block_ref, BlockReference::Finality(Finality::Optimistic));
    }

    #[test]
    fn test_block_reference_from_height() {
        let block_ref: BlockReference = 99999u64.into();
        assert_eq!(block_ref, BlockReference::Height(99999));
    }

    #[test]
    fn test_block_reference_from_hash() {
        let hash = CryptoHash::hash(b"block");
        let block_ref: BlockReference = hash.into();
        assert_eq!(block_ref, BlockReference::Hash(hash));
    }

    #[test]
    fn test_finality_default() {
        let default = Finality::default();
        assert_eq!(default, Finality::Final);
    }

    #[test]
    fn test_tx_execution_status_default() {
        let default = TxExecutionStatus::default();
        assert_eq!(default, TxExecutionStatus::ExecutedOptimistic);
    }

    #[test]
    fn test_tx_execution_status_all_variants() {
        assert_eq!(TxExecutionStatus::None.as_str(), "NONE");
        assert_eq!(TxExecutionStatus::Included.as_str(), "INCLUDED");
        assert_eq!(
            TxExecutionStatus::ExecutedOptimistic.as_str(),
            "EXECUTED_OPTIMISTIC"
        );
        assert_eq!(TxExecutionStatus::IncludedFinal.as_str(), "INCLUDED_FINAL");
        assert_eq!(TxExecutionStatus::Executed.as_str(), "EXECUTED");
        assert_eq!(TxExecutionStatus::Final.as_str(), "FINAL");
    }

    #[test]
    fn test_finality_serde_roundtrip() {
        // Test all variants serialize/deserialize correctly
        for finality in [Finality::Optimistic, Finality::NearFinal, Finality::Final] {
            let json = serde_json::to_string(&finality).unwrap();
            let parsed: Finality = serde_json::from_str(&json).unwrap();
            assert_eq!(finality, parsed);
        }
    }

    #[test]
    fn test_tx_execution_status_serde_roundtrip() {
        for status in [
            TxExecutionStatus::None,
            TxExecutionStatus::Included,
            TxExecutionStatus::ExecutedOptimistic,
            TxExecutionStatus::IncludedFinal,
            TxExecutionStatus::Executed,
            TxExecutionStatus::Final,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: TxExecutionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_block_reference_clone_and_eq() {
        let original = BlockReference::Height(12345);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_finality_clone_and_copy() {
        let f1 = Finality::Optimistic;
        let f2 = f1; // Copy
        #[allow(clippy::clone_on_copy)]
        let f3 = f1.clone(); // Clone (intentionally testing Clone impl)
        assert_eq!(f1, f2);
        assert_eq!(f1, f3);
    }
}
