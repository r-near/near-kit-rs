//! Additional RPC response types for validators, light client, and state changes.

use serde::Deserialize;

use super::rpc::{AccessKeyDetails, ValidatorStakeView};
use super::{AccountId, CryptoHash, NearToken, PublicKey, Signature};

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
    pub approvals_after_next: Vec<Option<Signature>>,
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
#[serde(rename_all = "snake_case", tag = "type")]
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
#[serde(rename_all = "snake_case", tag = "type", content = "change")]
pub enum StateChangeValueView {
    /// Account updated.
    AccountUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Account state fields (flattened: amount, locked, code_hash, etc.).
        #[serde(flatten)]
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
    /// Gas key nonce updated.
    GasKeyNonceUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Public key.
        public_key: PublicKey,
        /// Nonce index.
        index: u16,
        /// Nonce value.
        nonce: u64,
    },
    /// Data updated.
    DataUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Key (base64-encoded).
        #[serde(rename = "key_base64")]
        key: String,
        /// Value (base64-encoded).
        #[serde(rename = "value_base64")]
        value: String,
    },
    /// Data deleted.
    DataDeletion {
        /// Account ID.
        account_id: AccountId,
        /// Key (base64-encoded).
        #[serde(rename = "key_base64")]
        key: String,
    },
    /// Contract code updated.
    ContractCodeUpdate {
        /// Account ID.
        account_id: AccountId,
        /// Code (base64-encoded).
        #[serde(rename = "code_base64")]
        code: String,
    },
    /// Contract code deleted.
    ContractCodeDeletion {
        /// Account ID.
        account_id: AccountId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_key_nonce_update_deserialization() {
        let json = serde_json::json!({
            "type": "gas_key_nonce_update",
            "change": {
                "account_id": "alice.near",
                "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                "index": 3,
                "nonce": 42
            }
        });
        let change: StateChangeValueView = serde_json::from_value(json).unwrap();
        match change {
            StateChangeValueView::GasKeyNonceUpdate {
                account_id,
                public_key,
                index,
                nonce,
            } => {
                assert_eq!(account_id.as_str(), "alice.near");
                assert!(public_key.to_string().starts_with("ed25519:"));
                assert_eq!(index, 3);
                assert_eq!(nonce, 42);
            }
            _ => panic!("Expected GasKeyNonceUpdate"),
        }
    }

    #[test]
    fn test_state_change_with_cause_deserialization() {
        let json = serde_json::json!({
            "cause": {
                "type": "transaction_processing",
                "tx_hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"
            },
            "value": {
                "type": "gas_key_nonce_update",
                "change": {
                    "account_id": "alice.near",
                    "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                    "index": 0,
                    "nonce": 100
                }
            }
        });
        let change: StateChangeWithCauseView = serde_json::from_value(json).unwrap();
        assert!(matches!(
            change.value,
            StateChangeValueView::GasKeyNonceUpdate { .. }
        ));
    }

    #[test]
    fn test_state_change_cause_deserialization() {
        let json = serde_json::json!({
            "type": "transaction_processing",
            "tx_hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"
        });
        let cause: StateChangeCauseView = serde_json::from_value(json).unwrap();
        assert!(matches!(
            cause,
            StateChangeCauseView::TransactionProcessing { .. }
        ));
    }

    #[test]
    fn test_account_update_flattened_deserialization() {
        let json = serde_json::json!({
            "type": "account_update",
            "change": {
                "account_id": "alice.near",
                "amount": "1000000000000000000000000",
                "locked": "0",
                "code_hash": "11111111111111111111111111111111",
                "storage_usage": 100,
                "storage_paid_at": 0
            }
        });
        let change: StateChangeValueView = serde_json::from_value(json).unwrap();
        match change {
            StateChangeValueView::AccountUpdate {
                account_id,
                account,
            } => {
                assert_eq!(account_id.as_str(), "alice.near");
                assert_eq!(account["amount"], "1000000000000000000000000");
                assert_eq!(account["storage_usage"], 100);
            }
            _ => panic!("expected AccountUpdate"),
        }
    }

    #[test]
    fn test_data_update_base64_field_names() {
        let json = serde_json::json!({
            "type": "data_update",
            "change": {
                "account_id": "alice.near",
                "key_base64": "c3RhdGU=",
                "value_base64": "dGVzdA=="
            }
        });
        let change: StateChangeValueView = serde_json::from_value(json).unwrap();
        match change {
            StateChangeValueView::DataUpdate {
                account_id,
                key,
                value,
            } => {
                assert_eq!(account_id.as_str(), "alice.near");
                assert_eq!(key, "c3RhdGU=");
                assert_eq!(value, "dGVzdA==");
            }
            _ => panic!("expected DataUpdate"),
        }
    }

    #[test]
    fn test_data_deletion_base64_field_name() {
        let json = serde_json::json!({
            "type": "data_deletion",
            "change": {
                "account_id": "alice.near",
                "key_base64": "c3RhdGU="
            }
        });
        let change: StateChangeValueView = serde_json::from_value(json).unwrap();
        assert!(matches!(change, StateChangeValueView::DataDeletion { .. }));
    }

    #[test]
    fn test_contract_code_update_base64_field_name() {
        let json = serde_json::json!({
            "type": "contract_code_update",
            "change": {
                "account_id": "alice.near",
                "code_base64": "AGFzbQEAAAA="
            }
        });
        let change: StateChangeValueView = serde_json::from_value(json).unwrap();
        match change {
            StateChangeValueView::ContractCodeUpdate {
                account_id, code, ..
            } => {
                assert_eq!(account_id.as_str(), "alice.near");
                assert_eq!(code, "AGFzbQEAAAA=");
            }
            _ => panic!("expected ContractCodeUpdate"),
        }
    }
}
