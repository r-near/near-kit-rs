//! Additional RPC response types for validators.

use std::collections::HashMap;

use serde::Deserialize;

use super::rpc::ValidatorStakeView;
use super::{AccountId, NearToken, PublicKey};

// ============================================================================
// Validators / Epoch types
// ============================================================================

/// Epoch validator info from `validators` RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct EpochValidatorInfo {
    /// Current epoch validators.
    pub current_validators: Vec<CurrentEpochValidatorInfo>,
    /// Next epoch validators.
    pub next_validators: Vec<NextEpochValidatorInfo>,
    /// Current fishermen (deprecated, typically empty).
    #[serde(default)]
    pub current_fishermen: Vec<ValidatorStakeView>,
    /// Next fishermen (deprecated, typically empty).
    #[serde(default)]
    pub next_fishermen: Vec<ValidatorStakeView>,
    /// Current proposals for next epoch.
    #[serde(default)]
    pub current_proposals: Vec<ValidatorStakeView>,
    /// Validators kicked out in previous epoch.
    #[serde(default)]
    pub prev_epoch_kickout: Vec<ValidatorKickoutView>,
    /// Block height when the epoch started.
    pub epoch_start_height: u64,
    /// Epoch height.
    pub epoch_height: u64,
    /// Per-validator rewards paid out at the start of the previous epoch
    /// (rewards earned in epoch E-2). Empty on nodes older than nearcore 2.12.
    #[serde(default)]
    pub validator_reward_paid_prev_epoch: HashMap<AccountId, NearToken>,
}

/// Current epoch validator information.
#[derive(Debug, Clone, Deserialize)]
pub struct CurrentEpochValidatorInfo {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Validator public key.
    pub public_key: PublicKey,
    /// Whether this validator has been slashed.
    pub is_slashed: bool,
    /// Stake amount.
    pub stake: NearToken,
    /// Shards produced by this validator.
    #[serde(default)]
    pub shards_produced: Vec<u64>,
    /// Number of blocks produced.
    pub num_produced_blocks: u64,
    /// Number of blocks expected.
    pub num_expected_blocks: u64,
    /// Number of chunks produced.
    #[serde(default)]
    pub num_produced_chunks: u64,
    /// Number of chunks expected.
    #[serde(default)]
    pub num_expected_chunks: u64,
    /// Number of produced chunks per shard.
    #[serde(default)]
    pub num_produced_chunks_per_shard: Vec<u64>,
    /// Number of expected chunks per shard.
    #[serde(default)]
    pub num_expected_chunks_per_shard: Vec<u64>,
    /// Number of endorsements produced.
    #[serde(default)]
    pub num_produced_endorsements: u64,
    /// Number of endorsements expected.
    #[serde(default)]
    pub num_expected_endorsements: u64,
    /// Number of endorsements produced per shard.
    #[serde(default)]
    pub num_produced_endorsements_per_shard: Vec<u64>,
    /// Number of endorsements expected per shard.
    #[serde(default)]
    pub num_expected_endorsements_per_shard: Vec<u64>,
    /// Shards endorsed.
    #[serde(default)]
    pub shards_endorsed: Vec<u64>,
}

/// Next epoch validator information.
#[derive(Debug, Clone, Deserialize)]
pub struct NextEpochValidatorInfo {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Validator public key.
    pub public_key: PublicKey,
    /// Stake amount.
    pub stake: NearToken,
    /// Shards to be assigned.
    #[serde(default)]
    pub shards: Vec<u64>,
}

/// Validator kickout information.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorKickoutView {
    /// Account ID of the kicked validator.
    pub account_id: AccountId,
    /// Reason for kickout.
    pub reason: ValidatorKickoutReason,
}

/// Reason a validator was kicked out.
#[derive(Debug, Clone, Deserialize)]
pub enum ValidatorKickoutReason {
    /// Slashed (deprecated, unused).
    Slashed,
    /// Not enough blocks produced.
    NotEnoughBlocks {
        /// Blocks produced.
        produced: u64,
        /// Blocks expected.
        expected: u64,
    },
    /// Not enough chunks produced.
    NotEnoughChunks {
        /// Chunks produced.
        produced: u64,
        /// Chunks expected.
        expected: u64,
    },
    /// Validator unstaked.
    Unstaked,
    /// Not enough stake.
    NotEnoughStake {
        /// Validator's stake.
        stake: NearToken,
        /// Minimum threshold.
        threshold: NearToken,
    },
    /// Did not get a seat.
    DidNotGetASeat,
    /// Not enough chunk endorsements.
    NotEnoughChunkEndorsements {
        /// Endorsements produced.
        produced: u64,
        /// Endorsements expected.
        expected: u64,
    },
    /// Protocol version too old.
    ProtocolVersionTooOld {
        /// Validator's protocol version.
        version: u32,
        /// Network protocol version.
        network_version: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_validator_info_validator_reward_paid_prev_epoch() {
        let json = serde_json::json!({
            "current_validators": [],
            "next_validators": [],
            "epoch_start_height": 100,
            "epoch_height": 10,
            "validator_reward_paid_prev_epoch": {
                "alice.near": "1000000000000000000000000"
            }
        });
        let info: EpochValidatorInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info.validator_reward_paid_prev_epoch.len(), 1);
        let alice: AccountId = "alice.near".parse().unwrap();
        assert_eq!(
            info.validator_reward_paid_prev_epoch.get(&alice),
            Some(&NearToken::from_yoctonear(
                1_000_000_000_000_000_000_000_000
            ))
        );
    }

    #[test]
    fn test_epoch_validator_info_validator_reward_paid_prev_epoch_default_empty() {
        let json = serde_json::json!({
            "current_validators": [],
            "next_validators": [],
            "epoch_start_height": 100,
            "epoch_height": 10
        });
        let info: EpochValidatorInfo = serde_json::from_value(json).unwrap();
        assert!(info.validator_reward_paid_prev_epoch.is_empty());
    }
}
