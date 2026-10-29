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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The latest final block ([`Finality::Final`]). `View`s and `Near` reads use
/// it unless `.at(..)` says otherwise.
impl Default for BlockReference {
    fn default() -> Self {
        Self::Finality(Finality::Final)
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

impl From<SyncCheckpoint> for BlockReference {
    fn from(checkpoint: SyncCheckpoint) -> Self {
        Self::SyncCheckpoint(checkpoint)
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
///
/// This mirrors the RPC's values, and its `Default` (`ExecutedOptimistic`) is
/// the RPC's default when a request names none. near-kit's sends default to
/// `Final` instead (see `near_kit::transaction::Final`).
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
    fn wire_strings() {
        for (finality, wire) in [
            (Finality::Optimistic, "optimistic"),
            (Finality::NearFinal, "near-final"),
            (Finality::Final, "final"),
        ] {
            assert_eq!(finality.as_str(), wire);
            assert_eq!(serde_json::to_value(finality).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<Finality>(wire.into()).unwrap(),
                finality
            );
        }
        for (status, wire) in [
            (TxExecutionStatus::None, "NONE"),
            (TxExecutionStatus::Included, "INCLUDED"),
            (TxExecutionStatus::ExecutedOptimistic, "EXECUTED_OPTIMISTIC"),
            (TxExecutionStatus::IncludedFinal, "INCLUDED_FINAL"),
            (TxExecutionStatus::Executed, "EXECUTED"),
            (TxExecutionStatus::Final, "FINAL"),
        ] {
            assert_eq!(status.as_str(), wire);
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<TxExecutionStatus>(wire.into()).unwrap(),
                status
            );
        }
        for (checkpoint, wire) in [
            (SyncCheckpoint::Genesis, "genesis"),
            (SyncCheckpoint::EarliestAvailable, "earliest_available"),
        ] {
            assert_eq!(serde_json::to_value(checkpoint).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<SyncCheckpoint>(wire.into()).unwrap(),
                checkpoint
            );
        }
    }

    #[test]
    fn defaults_and_conversions() {
        assert_eq!(Finality::default(), Finality::Final);
        assert_eq!(
            TxExecutionStatus::default(),
            TxExecutionStatus::ExecutedOptimistic
        );
        assert_eq!(
            BlockReference::default(),
            BlockReference::Finality(Finality::Final)
        );

        let hash = CryptoHash::hash(b"block");
        assert_eq!(
            BlockReference::from(Finality::Optimistic),
            BlockReference::Finality(Finality::Optimistic)
        );
        assert_eq!(BlockReference::from(99_999), BlockReference::Height(99_999));
        assert_eq!(BlockReference::from(hash), BlockReference::Hash(hash));
        assert_eq!(
            BlockReference::from(SyncCheckpoint::Genesis),
            BlockReference::SyncCheckpoint(SyncCheckpoint::Genesis)
        );
    }
}
