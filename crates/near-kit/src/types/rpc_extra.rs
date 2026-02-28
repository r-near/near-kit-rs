//! Additional RPC response types for validators, light client, and state changes.

use serde::Deserialize;

use super::rpc::{AccessKeyDetails, ValidatorStakeView};
use super::{AccountId, CryptoHash, NearToken, PublicKey};

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
    #[serde(rename = "Slashed")]
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

// ============================================================================
// Light client types
// ============================================================================

/// Light client block view from `next_light_client_block` RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct LightClientBlockView {
    /// Previous block hash.
    pub prev_block_hash: CryptoHash,
    /// Next block inner hash.
    pub next_block_inner_hash: CryptoHash,
    /// Inner lite header.
    pub inner_lite: BlockHeaderInnerLiteView,
    /// Hash of inner rest fields.
    pub inner_rest_hash: CryptoHash,
    /// Next epoch block producers (None if unchanged).
    #[serde(default)]
    pub next_bps: Option<Vec<ValidatorStakeView>>,
    /// Approvals after next block.
    #[serde(default)]
    pub approvals_after_next: Vec<Option<String>>,
}

/// Light client block lite view.
#[derive(Debug, Clone, Deserialize)]
pub struct LightClientBlockLiteView {
    /// Previous block hash.
    pub prev_block_hash: CryptoHash,
    /// Hash of inner rest fields.
    pub inner_rest_hash: CryptoHash,
    /// Inner lite header.
    pub inner_lite: BlockHeaderInnerLiteView,
}

/// Block header inner lite (for light client proofs).
#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeaderInnerLiteView {
    /// Block height.
    pub height: u64,
    /// Epoch ID.
    pub epoch_id: CryptoHash,
    /// Next epoch ID.
    pub next_epoch_id: CryptoHash,
    /// Previous state root.
    pub prev_state_root: CryptoHash,
    /// Outcome root.
    pub outcome_root: CryptoHash,
    /// Timestamp (legacy, as u64).
    pub timestamp: u64,
    /// Timestamp in nanoseconds.
    pub timestamp_nanosec: String,
    /// Next block producers hash.
    pub next_bp_hash: CryptoHash,
    /// Block merkle root.
    pub block_merkle_root: CryptoHash,
}

// ============================================================================
// State change types
// ============================================================================

/// State change with its cause (from `EXPERIMENTAL_changes` RPC).
#[derive(Debug, Clone, Deserialize)]
pub struct StateChangeWithCauseView {
    /// What caused this state change.
    pub cause: StateChangeCauseView,
    /// The state change value.
    pub value: StateChangeValueView,
}

/// Cause of a state change.
#[derive(Debug, Clone, Deserialize)]
pub enum StateChangeCauseView {
    /// State not writable to disk.
    NotWritableToDisk,
    /// Initial state.
    InitialState,
    /// Transaction processing.
    TransactionProcessing {
        /// Transaction hash.
        tx_hash: CryptoHash,
    },
    /// Action receipt processing started.
    ActionReceiptProcessingStarted {
        /// Receipt hash.
        receipt_hash: CryptoHash,
    },
    /// Action receipt gas reward.
    ActionReceiptGasReward {
        /// Receipt hash.
        receipt_hash: CryptoHash,
    },
    /// Receipt processing.
    ReceiptProcessing {
        /// Receipt hash.
        receipt_hash: CryptoHash,
    },
    /// Postponed receipt.
    PostponedReceipt {
        /// Receipt hash.
        receipt_hash: CryptoHash,
    },
    /// Updated delayed receipts.
    UpdatedDelayedReceipts,
    /// Validator accounts update.
    ValidatorAccountsUpdate,
    /// State migration.
    Migration,
    /// Bandwidth scheduler state update.
    BandwidthSchedulerStateUpdate,
}

/// State change value.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChangeValueView {
    /// Account updated.
    AccountUpdate {
        /// Account ID.
        account_id: AccountId,
        /// New account state (as JSON value to avoid circular dependency).
        account: serde_json::Value,
    },
    /// Account deleted.
    AccountDeletion {
        /// Account ID.
        account_id: AccountId,
    },
    /// Access key updated.
    AccessKeyUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Public key.
        public_key: PublicKey,
        /// New access key.
        access_key: AccessKeyDetails,
    },
    /// Access key deleted.
    AccessKeyDeletion {
        /// Account ID.
        account_id: AccountId,
        /// Public key.
        public_key: PublicKey,
    },
    /// Data updated.
    DataUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Key (base64).
        key: String,
        /// Value (base64).
        value: String,
    },
    /// Data deleted.
    DataDeletion {
        /// Account ID.
        account_id: AccountId,
        /// Key (base64).
        key: String,
    },
    /// Contract code updated.
    ContractCodeUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Code (base64).
        code: String,
    },
    /// Contract code deleted.
    ContractCodeDeletion {
        /// Account ID.
        account_id: AccountId,
    },
}
