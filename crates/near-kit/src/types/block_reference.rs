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

    /// Returns `true` if all non-refund receipt outcomes are available.
    ///
    /// True for `ExecutedOptimistic`, `Executed`, and `Final`.
    pub fn is_executed(&self) -> bool {
        matches!(
            self,
            Self::ExecutedOptimistic | Self::Executed | Self::Final
        )
    }

    /// Returns `true` if the transaction's block has reached finality.
    ///
    /// True for `IncludedFinal`, `Executed`, and `Final`.
    pub fn is_block_final(&self) -> bool {
        matches!(self, Self::IncludedFinal | Self::Executed | Self::Final)
    }

    /// Returns `true` if all receipts are executed and all blocks finalized.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Final)
    }
}

/// Partial ordering for `TxExecutionStatus` forms a diamond lattice:
///
/// ```text
///         None
///          |
///       Included
///       /      \
/// ExecutedOptimistic  IncludedFinal
///       \      /
///       Executed
///          |
///        Final
/// ```
///
/// `ExecutedOptimistic` and `IncludedFinal` are **incomparable** because they
/// represent progress on orthogonal axes (execution vs block finality).
impl PartialOrd for TxExecutionStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use TxExecutionStatus::*;
        use std::cmp::Ordering::*;

        if self == other {
            return Some(Equal);
        }

        // Assign each variant a position in the lattice.
        // Returns (execution_level, finality_level).
        fn axes(s: &TxExecutionStatus) -> (u8, u8) {
            match s {
                None => (0, 0),
                Included => (1, 1),
                ExecutedOptimistic => (2, 1),
                IncludedFinal => (1, 2),
                Executed => (2, 2),
                Final => (3, 3),
            }
        }

        let (ex_a, fin_a) = axes(self);
        let (ex_b, fin_b) = axes(other);

        match (ex_a.cmp(&ex_b), fin_a.cmp(&fin_b)) {
            (Equal, Equal) => Some(Equal),
            (Less | Equal, Less | Equal) => Some(Less),
            (Greater | Equal, Greater | Equal) => Some(Greater),
            _ => Option::None, // incomparable
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

    #[test]
    fn test_sync_checkpoint_constructors() {
        let genesis = BlockReference::genesis();
        assert!(matches!(
            genesis,
            BlockReference::SyncCheckpoint(SyncCheckpoint::Genesis)
        ));

        let earliest = BlockReference::earliest_available();
        assert!(matches!(
            earliest,
            BlockReference::SyncCheckpoint(SyncCheckpoint::EarliestAvailable)
        ));
    }

    #[test]
    fn test_sync_checkpoint_rpc_params() {
        let genesis = BlockReference::genesis();
        let params = genesis.to_rpc_params();
        assert_eq!(params["sync_checkpoint"], "genesis");

        let earliest = BlockReference::earliest_available();
        let params = earliest.to_rpc_params();
        assert_eq!(params["sync_checkpoint"], "earliest_available");
    }

    #[test]
    fn test_sync_checkpoint_serde_roundtrip() {
        for cp in [SyncCheckpoint::Genesis, SyncCheckpoint::EarliestAvailable] {
            let json = serde_json::to_string(&cp).unwrap();
            let parsed: SyncCheckpoint = serde_json::from_str(&json).unwrap();
            assert_eq!(cp, parsed);
        }
    }

    #[test]
    fn test_tx_execution_status_is_executed() {
        assert!(!TxExecutionStatus::None.is_executed());
        assert!(!TxExecutionStatus::Included.is_executed());
        assert!(TxExecutionStatus::ExecutedOptimistic.is_executed());
        assert!(!TxExecutionStatus::IncludedFinal.is_executed());
        assert!(TxExecutionStatus::Executed.is_executed());
        assert!(TxExecutionStatus::Final.is_executed());
    }

    #[test]
    fn test_tx_execution_status_is_block_final() {
        assert!(!TxExecutionStatus::None.is_block_final());
        assert!(!TxExecutionStatus::Included.is_block_final());
        assert!(!TxExecutionStatus::ExecutedOptimistic.is_block_final());
        assert!(TxExecutionStatus::IncludedFinal.is_block_final());
        assert!(TxExecutionStatus::Executed.is_block_final());
        assert!(TxExecutionStatus::Final.is_block_final());
    }

    #[test]
    fn test_tx_execution_status_is_final() {
        assert!(!TxExecutionStatus::None.is_final());
        assert!(!TxExecutionStatus::Included.is_final());
        assert!(!TxExecutionStatus::ExecutedOptimistic.is_final());
        assert!(!TxExecutionStatus::IncludedFinal.is_final());
        assert!(!TxExecutionStatus::Executed.is_final());
        assert!(TxExecutionStatus::Final.is_final());
    }

    #[test]
    fn test_tx_execution_status_partial_ord_linear() {
        // Linear chain: None < Included < Executed < Final
        assert!(TxExecutionStatus::None < TxExecutionStatus::Included);
        assert!(TxExecutionStatus::Included < TxExecutionStatus::Executed);
        assert!(TxExecutionStatus::Executed < TxExecutionStatus::Final);
        assert!(TxExecutionStatus::None < TxExecutionStatus::Final);
    }

    #[test]
    fn test_tx_execution_status_partial_ord_branches() {
        // Both branches are greater than Included
        assert!(TxExecutionStatus::ExecutedOptimistic > TxExecutionStatus::Included);
        assert!(TxExecutionStatus::IncludedFinal > TxExecutionStatus::Included);
        // Both branches are less than Executed
        assert!(TxExecutionStatus::ExecutedOptimistic < TxExecutionStatus::Executed);
        assert!(TxExecutionStatus::IncludedFinal < TxExecutionStatus::Executed);
    }

    #[test]
    fn test_tx_execution_status_partial_ord_incomparable() {
        // ExecutedOptimistic and IncludedFinal are incomparable
        assert_eq!(
            TxExecutionStatus::ExecutedOptimistic.partial_cmp(&TxExecutionStatus::IncludedFinal),
            Option::None,
        );
        assert_eq!(
            TxExecutionStatus::IncludedFinal.partial_cmp(&TxExecutionStatus::ExecutedOptimistic),
            Option::None,
        );
        // Neither is >, <, ==
        assert_ne!(
            TxExecutionStatus::ExecutedOptimistic,
            TxExecutionStatus::IncludedFinal
        );
    }
}
