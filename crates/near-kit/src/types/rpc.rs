//! RPC response types.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;

use super::block_reference::TxExecutionStatus;
use super::error::TxExecutionError;
use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey, Signature};

// ============================================================================
// Constants
// ============================================================================

/// Cost per byte of storage in yoctoNEAR.
///
/// This is a protocol constant (10^19 yoctoNEAR per byte = 0.00001 NEAR/byte).
/// It has remained unchanged since NEAR genesis and would require a hard fork
/// to modify. Used for calculating available balance.
///
/// See: <https://docs.near.org/concepts/storage/storage-staking>
pub const STORAGE_AMOUNT_PER_BYTE: u128 = 10_000_000_000_000_000_000; // 10^19 yoctoNEAR

// ============================================================================
// Account types
// ============================================================================

/// Account information from view_account RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountView {
    /// Total balance including locked.
    pub amount: NearToken,
    /// Locked balance (staked).
    pub locked: NearToken,
    /// Hash of deployed contract code (or zeros if none).
    pub code_hash: CryptoHash,
    /// Storage used in bytes.
    pub storage_usage: u64,
    /// Storage paid at block height (deprecated, always 0).
    #[serde(default)]
    pub storage_paid_at: u64,
    /// Global contract code hash (if using a global contract).
    #[serde(default)]
    pub global_contract_hash: Option<CryptoHash>,
    /// Global contract account ID (if using a global contract by account).
    #[serde(default)]
    pub global_contract_account_id: Option<AccountId>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

impl AccountView {
    /// Calculate the total NEAR required for storage.
    fn storage_required(&self) -> NearToken {
        let yocto = STORAGE_AMOUNT_PER_BYTE.saturating_mul(self.storage_usage as u128);
        NearToken::from_yoctonear(yocto)
    }

    /// Get available (spendable) balance.
    ///
    /// This accounts for the protocol rule that staked tokens count towards
    /// the storage requirement:
    /// - available = amount - max(0, storage_required - locked)
    ///
    /// If staked >= storage cost, all liquid balance is available.
    /// If staked < storage cost, some liquid balance is reserved for storage.
    pub fn available(&self) -> NearToken {
        let storage_required = self.storage_required();

        // If staked covers storage, all liquid is available
        if self.locked >= storage_required {
            return self.amount;
        }

        // Otherwise, reserve the difference from liquid balance
        let reserved_for_storage = storage_required.saturating_sub(self.locked);
        self.amount.saturating_sub(reserved_for_storage)
    }

    /// Get the amount of NEAR reserved for storage costs.
    ///
    /// This is calculated as: max(0, storage_required - locked)
    pub fn storage_cost(&self) -> NearToken {
        let storage_required = self.storage_required();

        if self.locked >= storage_required {
            NearToken::ZERO
        } else {
            storage_required.saturating_sub(self.locked)
        }
    }

    /// Check if this account has a deployed contract.
    pub fn has_contract(&self) -> bool {
        !self.code_hash.is_zero()
    }
}

/// Simplified balance info.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    /// Total balance (available + locked).
    pub total: NearToken,
    /// Available balance (spendable, accounting for storage).
    pub available: NearToken,
    /// Locked balance (staked).
    pub locked: NearToken,
    /// Amount reserved for storage costs.
    pub storage_cost: NearToken,
    /// Storage used in bytes.
    pub storage_usage: u64,
}

impl std::fmt::Display for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.available)
    }
}

impl From<AccountView> for AccountBalance {
    fn from(view: AccountView) -> Self {
        Self {
            total: view.amount,
            available: view.available(),
            locked: view.locked,
            storage_cost: view.storage_cost(),
            storage_usage: view.storage_usage,
        }
    }
}

/// Access key information from view_access_key RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyView {
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Permission level.
    pub permission: AccessKeyPermissionView,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// Access key details (without block info, used in lists).
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyDetails {
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Permission level.
    pub permission: AccessKeyPermissionView,
}

/// Access key permission from RPC.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AccessKeyPermissionView {
    /// Full access.
    FullAccess,
    /// Function call access with restrictions.
    FunctionCall {
        /// Maximum amount this key can spend.
        allowance: Option<NearToken>,
        /// Contract that can be called.
        receiver_id: AccountId,
        /// Methods that can be called (empty = all).
        method_names: Vec<String>,
    },
    /// Gas key with function call access.
    GasKeyFunctionCall {
        /// Gas key balance.
        balance: NearToken,
        /// Number of nonces.
        num_nonces: u16,
        /// Maximum amount this key can spend.
        allowance: Option<NearToken>,
        /// Contract that can be called.
        receiver_id: AccountId,
        /// Methods that can be called (empty = all).
        method_names: Vec<String>,
    },
    /// Gas key with full access.
    GasKeyFullAccess {
        /// Gas key balance.
        balance: NearToken,
        /// Number of nonces.
        num_nonces: u16,
    },
}

/// Access key list from view_access_key_list RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyListView {
    /// List of access keys.
    pub keys: Vec<AccessKeyInfoView>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// Single access key info in list.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyInfoView {
    /// Public key.
    pub public_key: PublicKey,
    /// Access key details.
    pub access_key: AccessKeyDetails,
}

// ============================================================================
// Block types
// ============================================================================

/// Block information from block RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockView {
    /// Block author (validator account ID).
    pub author: AccountId,
    /// Block header.
    pub header: BlockHeaderView,
    /// List of chunks in the block.
    pub chunks: Vec<ChunkHeaderView>,
}

/// Block header with full details.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeaderView {
    /// Block height.
    pub height: u64,
    /// Previous block height (may be None for genesis).
    #[serde(default)]
    pub prev_height: Option<u64>,
    /// Block hash.
    pub hash: CryptoHash,
    /// Previous block hash.
    pub prev_hash: CryptoHash,
    /// Previous state root.
    pub prev_state_root: CryptoHash,
    /// Chunk receipts root.
    pub chunk_receipts_root: CryptoHash,
    /// Chunk headers root.
    pub chunk_headers_root: CryptoHash,
    /// Chunk transaction root.
    pub chunk_tx_root: CryptoHash,
    /// Outcome root.
    pub outcome_root: CryptoHash,
    /// Number of chunks included.
    pub chunks_included: u64,
    /// Challenges root.
    pub challenges_root: CryptoHash,
    /// Timestamp in nanoseconds (as u64).
    pub timestamp: u64,
    /// Timestamp in nanoseconds (as string for precision).
    pub timestamp_nanosec: String,
    /// Random value for the block.
    pub random_value: CryptoHash,
    /// Validator proposals.
    #[serde(default)]
    pub validator_proposals: Vec<ValidatorStakeView>,
    /// Chunk mask (which shards have chunks).
    #[serde(default)]
    pub chunk_mask: Vec<bool>,
    /// Gas price for this block.
    pub gas_price: NearToken,
    /// Block ordinal (may be None).
    #[serde(default)]
    pub block_ordinal: Option<u64>,
    /// Total supply of NEAR tokens.
    pub total_supply: NearToken,
    /// Challenges result.
    #[serde(default)]
    pub challenges_result: Vec<SlashedValidator>,
    /// Last final block hash.
    pub last_final_block: CryptoHash,
    /// Last DS final block hash.
    pub last_ds_final_block: CryptoHash,
    /// Epoch ID.
    pub epoch_id: CryptoHash,
    /// Next epoch ID.
    pub next_epoch_id: CryptoHash,
    /// Next block producer hash.
    pub next_bp_hash: CryptoHash,
    /// Block merkle root.
    pub block_merkle_root: CryptoHash,
    /// Epoch sync data hash (optional).
    #[serde(default)]
    pub epoch_sync_data_hash: Option<CryptoHash>,
    /// Block body hash (optional, added in later protocol versions).
    #[serde(default)]
    pub block_body_hash: Option<CryptoHash>,
    /// Block approvals (nullable signatures).
    #[serde(default)]
    pub approvals: Vec<Option<Signature>>,
    /// Block signature.
    pub signature: Signature,
    /// Latest protocol version.
    pub latest_protocol_version: u32,
    /// Rent paid (deprecated; when present, always 0).
    #[serde(default)]
    pub rent_paid: Option<NearToken>,
    /// Validator reward (deprecated; when present, always 0).
    #[serde(default)]
    pub validator_reward: Option<NearToken>,
    /// Chunk endorsements (optional).
    #[serde(default)]
    pub chunk_endorsements: Option<Vec<Vec<u8>>>,
    /// Shard split info (optional).
    #[serde(default)]
    pub shard_split: Option<serde_json::Value>,
}

/// Validator stake (versioned).
///
/// Used for validator proposals in block/chunk headers.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ValidatorStakeView {
    /// Version 1 (current).
    V1(ValidatorStakeViewV1),
}

/// Validator stake data.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorStakeViewV1 {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Public key.
    pub public_key: PublicKey,
    /// Stake amount.
    pub stake: NearToken,
}

impl ValidatorStakeView {
    /// Get the inner V1 data.
    pub fn into_v1(self) -> ValidatorStakeViewV1 {
        match self {
            Self::V1(v) => v,
        }
    }

    /// Get the account ID.
    pub fn account_id(&self) -> &AccountId {
        match self {
            Self::V1(v) => &v.account_id,
        }
    }

    /// Get the stake amount.
    pub fn stake(&self) -> NearToken {
        match self {
            Self::V1(v) => v.stake,
        }
    }
}

/// Slashed validator from challenge results.
#[derive(Debug, Clone, Deserialize)]
pub struct SlashedValidator {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Whether this was a double sign.
    pub is_double_sign: bool,
}

/// Chunk header with full details.
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkHeaderView {
    /// Chunk hash.
    pub chunk_hash: CryptoHash,
    /// Previous block hash.
    pub prev_block_hash: CryptoHash,
    /// Outcome root.
    pub outcome_root: CryptoHash,
    /// Previous state root.
    pub prev_state_root: CryptoHash,
    /// Encoded merkle root.
    pub encoded_merkle_root: CryptoHash,
    /// Encoded length.
    pub encoded_length: u64,
    /// Height when chunk was created.
    pub height_created: u64,
    /// Height when chunk was included.
    pub height_included: u64,
    /// Shard ID.
    pub shard_id: u64,
    /// Gas used in this chunk.
    pub gas_used: u64,
    /// Gas limit for this chunk.
    pub gas_limit: u64,
    /// Validator reward.
    pub validator_reward: NearToken,
    /// Balance burnt.
    pub balance_burnt: NearToken,
    /// Outgoing receipts root.
    pub outgoing_receipts_root: CryptoHash,
    /// Transaction root.
    pub tx_root: CryptoHash,
    /// Validator proposals.
    #[serde(default)]
    pub validator_proposals: Vec<ValidatorStakeView>,
    /// Congestion info (optional, added in later protocol versions).
    #[serde(default)]
    pub congestion_info: Option<CongestionInfoView>,
    /// Bandwidth requests (optional, added in later protocol versions).
    #[serde(default)]
    pub bandwidth_requests: Option<serde_json::Value>,
    /// Rent paid (deprecated; when present, always 0).
    #[serde(default)]
    pub rent_paid: Option<NearToken>,
    /// Proposed split (optional).
    #[serde(default)]
    pub proposed_split: Option<serde_json::Value>,
    /// Chunk signature.
    pub signature: Signature,
}

/// Congestion information for a shard.
#[derive(Debug, Clone, Deserialize)]
pub struct CongestionInfoView {
    /// Gas used by delayed receipts.
    #[serde(default, deserialize_with = "dec_format")]
    pub delayed_receipts_gas: u128,
    /// Gas used by buffered receipts.
    #[serde(default, deserialize_with = "dec_format")]
    pub buffered_receipts_gas: u128,
    /// Bytes used by receipts.
    #[serde(default)]
    pub receipt_bytes: u64,
    /// Allowed shard.
    #[serde(default)]
    pub allowed_shard: u16,
}

/// Deserialize a u128 from a decimal string (NEAR RPC sends u128 as strings).
fn dec_format<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNum {
        String(String),
        Num(u128),
    }
    match StringOrNum::deserialize(deserializer)? {
        StringOrNum::String(s) => s.parse().map_err(serde::de::Error::custom),
        StringOrNum::Num(n) => Ok(n),
    }
}

/// Gas price response.
#[derive(Debug, Clone, Deserialize)]
pub struct GasPrice {
    /// Gas price in yoctoNEAR.
    pub gas_price: NearToken,
}

impl GasPrice {
    /// Get gas price as u128.
    pub fn as_u128(&self) -> u128 {
        self.gas_price.as_yoctonear()
    }
}

// ============================================================================
// Transaction outcome types
// ============================================================================

/// Overall transaction execution status.
///
/// Represents the final result of a transaction, matching nearcore's `FinalExecutionStatus`.
/// This is the `status` field in `FinalExecutionOutcome`.
#[derive(Debug, Clone, Default, Deserialize)]
pub enum FinalExecutionStatus {
    /// The transaction has not yet started execution.
    #[default]
    NotStarted,
    /// The transaction has started but the first receipt hasn't completed.
    Started,
    /// The transaction execution failed.
    Failure(TxExecutionError),
    /// The transaction execution succeeded (base64-encoded return value).
    SuccessValue(String),
}

/// Final execution outcome from send_tx RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcome {
    /// The wait level that was reached (e.g. `EXECUTED_OPTIMISTIC`, `FINAL`).
    pub final_execution_status: TxExecutionStatus,
    /// Overall transaction execution result.
    #[serde(default)]
    pub status: FinalExecutionStatus,
    /// The transaction that was executed (full details for executed, minimal for pending).
    #[serde(default)]
    pub transaction: Option<TransactionView>,
    /// Outcome of the transaction itself (only for executed transactions).
    #[serde(default)]
    pub transaction_outcome: Option<ExecutionOutcomeWithId>,
    /// Outcomes of all receipts spawned by the transaction (only for executed transactions).
    #[serde(default)]
    pub receipts_outcome: Vec<ExecutionOutcomeWithId>,
}

impl FinalExecutionOutcome {
    /// Check if the transaction succeeded.
    pub fn is_success(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::SuccessValue(_))
    }

    /// Check if the transaction failed.
    pub fn is_failure(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::Failure(_))
    }

    /// Check if the transaction is still pending (not yet started or still executing).
    pub fn is_pending(&self) -> bool {
        matches!(
            self.status,
            FinalExecutionStatus::NotStarted | FinalExecutionStatus::Started
        )
    }

    /// Get the success value if present (base64 decoded).
    pub fn success_value(&self) -> Option<Vec<u8>> {
        match &self.status {
            FinalExecutionStatus::SuccessValue(s) => STANDARD.decode(s).ok(),
            _ => None,
        }
    }

    /// Get the success value as a string if present.
    pub fn success_value_string(&self) -> Option<String> {
        self.success_value().and_then(|v| String::from_utf8(v).ok())
    }

    /// Get the success value deserialized as JSON.
    pub fn success_value_json<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        self.success_value()
            .and_then(|v| serde_json::from_slice(&v).ok())
    }

    /// Get the failure message if present.
    pub fn failure_message(&self) -> Option<String> {
        match &self.status {
            FinalExecutionStatus::Failure(err) => Some(err.to_string()),
            _ => None,
        }
    }

    /// Get the typed execution error if present.
    pub fn failure_error(&self) -> Option<&TxExecutionError> {
        match &self.status {
            FinalExecutionStatus::Failure(err) => Some(err),
            _ => None,
        }
    }

    /// Get the transaction hash.
    pub fn transaction_hash(&self) -> Option<&CryptoHash> {
        self.transaction_outcome.as_ref().map(|o| &o.id)
    }

    /// Get total gas used across all receipts.
    pub fn total_gas_used(&self) -> Gas {
        let tx_gas = self
            .transaction_outcome
            .as_ref()
            .map(|o| o.outcome.gas_burnt.as_gas())
            .unwrap_or(0);
        let receipt_gas: u64 = self
            .receipts_outcome
            .iter()
            .map(|r| r.outcome.gas_burnt.as_gas())
            .sum();
        Gas::from_gas(tx_gas + receipt_gas)
    }
}

/// Per-receipt execution status.
///
/// Matches nearcore's `ExecutionStatusView`. Used in [`ExecutionOutcome`].
#[derive(Debug, Clone, Deserialize)]
pub enum ExecutionStatus {
    /// The execution is pending or unknown.
    Unknown,
    /// Execution failed.
    Failure(TxExecutionError),
    /// Execution succeeded with a return value (base64 encoded).
    SuccessValue(String),
    /// Execution succeeded, producing a receipt.
    SuccessReceiptId(CryptoHash),
}

/// Transaction view in outcome.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionView {
    /// Signer account.
    pub signer_id: AccountId,
    /// Signer public key.
    pub public_key: PublicKey,
    /// Transaction nonce.
    pub nonce: u64,
    /// Receiver account.
    pub receiver_id: AccountId,
    /// Transaction hash.
    pub hash: CryptoHash,
    /// Actions in the transaction.
    #[serde(default)]
    pub actions: Vec<ActionView>,
    /// Transaction signature.
    pub signature: Signature,
    /// Priority fee (optional, for congestion pricing).
    #[serde(default)]
    pub priority_fee: Option<u64>,
    /// Nonce index (for gas key multi-nonce support).
    #[serde(default)]
    pub nonce_index: Option<u16>,
}

// ============================================================================
// Global contract identifier view
// ============================================================================

/// Backward-compatible deserialization helper for `GlobalContractIdentifierView`.
///
/// Handles both the new format (`{"hash": "<base58>"}` / `{"account_id": "alice.near"}`)
/// and the deprecated format (bare string `"<base58>"` / `"alice.near"`).
#[derive(Deserialize)]
#[serde(untagged)]
enum GlobalContractIdCompat {
    CodeHash { hash: CryptoHash },
    AccountId { account_id: AccountId },
    DeprecatedCodeHash(CryptoHash),
    DeprecatedAccountId(AccountId),
}

/// Global contract identifier in RPC view responses.
///
/// Identifies a global contract either by its code hash (immutable) or by the
/// publishing account ID (updatable). Supports both the current and deprecated
/// JSON serialization formats from nearcore.
#[derive(Debug, Clone, Deserialize)]
#[serde(from = "GlobalContractIdCompat")]
pub enum GlobalContractIdentifierView {
    /// Referenced by code hash.
    CodeHash(CryptoHash),
    /// Referenced by publisher account ID.
    AccountId(AccountId),
}

impl From<GlobalContractIdCompat> for GlobalContractIdentifierView {
    fn from(compat: GlobalContractIdCompat) -> Self {
        match compat {
            GlobalContractIdCompat::CodeHash { hash }
            | GlobalContractIdCompat::DeprecatedCodeHash(hash) => Self::CodeHash(hash),
            GlobalContractIdCompat::AccountId { account_id }
            | GlobalContractIdCompat::DeprecatedAccountId(account_id) => {
                Self::AccountId(account_id)
            }
        }
    }
}

// ============================================================================
// Action view
// ============================================================================

/// Action view in transaction.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ActionView {
    CreateAccount,
    DeployContract {
        code: String, // base64
    },
    FunctionCall {
        method_name: String,
        args: String, // base64
        gas: Gas,
        deposit: NearToken,
    },
    Transfer {
        deposit: NearToken,
    },
    Stake {
        stake: NearToken,
        public_key: PublicKey,
    },
    AddKey {
        public_key: PublicKey,
        access_key: AccessKeyDetails,
    },
    DeleteKey {
        public_key: PublicKey,
    },
    DeleteAccount {
        beneficiary_id: AccountId,
    },
    Delegate {
        delegate_action: serde_json::Value,
        signature: Signature,
    },
    #[serde(rename = "DeployGlobalContract")]
    DeployGlobalContract {
        code: String,
    },
    #[serde(rename = "DeployGlobalContractByAccountId")]
    DeployGlobalContractByAccountId {
        code: String,
    },
    #[serde(rename = "UseGlobalContract")]
    UseGlobalContract {
        code_hash: CryptoHash,
    },
    #[serde(rename = "UseGlobalContractByAccountId")]
    UseGlobalContractByAccountId {
        account_id: AccountId,
    },
    #[serde(rename = "DeterministicStateInit")]
    DeterministicStateInit {
        code: GlobalContractIdentifierView,
        #[serde(default)]
        data: BTreeMap<String, String>,
        deposit: NearToken,
    },
    TransferToGasKey {
        public_key: PublicKey,
        deposit: NearToken,
    },
    WithdrawFromGasKey {
        public_key: PublicKey,
        amount: NearToken,
    },
}

/// Merkle path item for cryptographic proofs.
#[derive(Debug, Clone, Deserialize)]
pub struct MerklePathItem {
    /// Hash at this node.
    pub hash: CryptoHash,
    /// Direction of the path.
    pub direction: MerkleDirection,
}

/// Direction in merkle path.
#[derive(Debug, Clone, Deserialize)]
pub enum MerkleDirection {
    Left,
    Right,
}

/// Execution outcome with ID.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionOutcomeWithId {
    /// Receipt or transaction ID.
    pub id: CryptoHash,
    /// Outcome details.
    pub outcome: ExecutionOutcome,
    /// Proof of execution.
    #[serde(default)]
    pub proof: Vec<MerklePathItem>,
    /// Block hash where this was executed.
    pub block_hash: CryptoHash,
}

/// Execution outcome details.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionOutcome {
    /// Executor account.
    pub executor_id: AccountId,
    /// Gas burnt during execution.
    pub gas_burnt: Gas,
    /// Tokens burnt for gas.
    pub tokens_burnt: NearToken,
    /// Logs emitted.
    pub logs: Vec<String>,
    /// Receipt IDs generated.
    pub receipt_ids: Vec<CryptoHash>,
    /// Execution status.
    pub status: ExecutionStatus,
    /// Execution metadata (gas profiling).
    #[serde(default)]
    pub metadata: Option<ExecutionMetadata>,
}

/// Execution metadata with gas profiling.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionMetadata {
    /// Metadata version.
    pub version: u32,
    /// Gas profile entries.
    #[serde(default)]
    pub gas_profile: Option<Vec<GasProfileEntry>>,
}

/// Gas profile entry for detailed gas accounting.
#[derive(Debug, Clone, Deserialize)]
pub struct GasProfileEntry {
    /// Cost category (ACTION_COST or WASM_HOST_COST).
    pub cost_category: String,
    /// Cost name.
    pub cost: String,
    /// Gas used for this cost (decimal string).
    pub gas_used: String,
}

/// View function result from call_function RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct ViewFunctionResult {
    /// Result bytes (often JSON).
    pub result: Vec<u8>,
    /// Logs emitted during view call.
    pub logs: Vec<String>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

impl ViewFunctionResult {
    /// Get the result as raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.result
    }

    /// Get the result as a string.
    pub fn as_string(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.result.clone())
    }

    /// Deserialize the result as JSON.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = rpc.view_function(&contract, "get_data", &[], block).await?;
    /// let data: MyData = result.json()?;
    /// ```
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.result)
    }

    /// Deserialize the result as Borsh.
    ///
    /// Use this for contracts that return Borsh-encoded data instead of JSON.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = rpc.view_function(&contract, "get_state", &args, block).await?;
    /// let state: ContractState = result.borsh()?;
    /// ```
    pub fn borsh<T: borsh::BorshDeserialize>(&self) -> Result<T, borsh::io::Error> {
        borsh::from_slice(&self.result)
    }
}

// ============================================================================
// Receipt types (for EXPERIMENTAL_tx_status)
// ============================================================================

/// Receipt from EXPERIMENTAL_tx_status.
#[derive(Debug, Clone, Deserialize)]
pub struct Receipt {
    /// Predecessor account that created this receipt.
    pub predecessor_id: AccountId,
    /// Receiver account for this receipt.
    pub receiver_id: AccountId,
    /// Receipt ID.
    pub receipt_id: CryptoHash,
    /// Receipt content (action or data).
    pub receipt: ReceiptContent,
    /// Priority (optional, for congestion pricing).
    #[serde(default)]
    pub priority: Option<u64>,
}

/// Receipt content - action, data, or global contract distribution.
#[derive(Debug, Clone, Deserialize)]
pub enum ReceiptContent {
    /// Action receipt.
    Action(ActionReceiptData),
    /// Data receipt.
    Data(DataReceiptData),
    /// Global contract distribution receipt.
    GlobalContractDistribution {
        /// Global contract identifier.
        id: GlobalContractIdentifierView,
        /// Target shard ID.
        target_shard: u64,
        /// Shards that have already received this contract.
        #[serde(default)]
        already_delivered_shards: Vec<u64>,
        /// Code bytes (base64).
        code: String,
        /// Nonce (present in v2 receipts).
        #[serde(default)]
        nonce: Option<u64>,
    },
}

/// Data receiver for output data in action receipts.
#[derive(Debug, Clone, Deserialize)]
pub struct DataReceiverView {
    /// Data ID.
    pub data_id: CryptoHash,
    /// Receiver account ID.
    pub receiver_id: AccountId,
}

/// Action receipt data.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionReceiptData {
    /// Signer account ID.
    pub signer_id: AccountId,
    /// Signer public key.
    pub signer_public_key: PublicKey,
    /// Gas price for this receipt.
    pub gas_price: NearToken,
    /// Output data receivers.
    #[serde(default)]
    pub output_data_receivers: Vec<DataReceiverView>,
    /// Input data IDs.
    #[serde(default)]
    pub input_data_ids: Vec<CryptoHash>,
    /// Actions in this receipt.
    pub actions: Vec<ActionView>,
    /// Whether this is a promise yield.
    #[serde(default)]
    pub is_promise_yield: Option<bool>,
}

/// Data receipt data.
#[derive(Debug, Clone, Deserialize)]
pub struct DataReceiptData {
    /// Data ID.
    pub data_id: CryptoHash,
    /// Data content (optional).
    #[serde(default)]
    pub data: Option<String>,
}

/// Final execution outcome with receipts (from EXPERIMENTAL_tx_status).
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcomeWithReceipts {
    /// The wait level that was reached (e.g. `EXECUTED_OPTIMISTIC`, `FINAL`).
    pub final_execution_status: TxExecutionStatus,
    /// Overall transaction execution result.
    #[serde(default)]
    pub status: FinalExecutionStatus,
    /// The transaction that was executed.
    #[serde(default)]
    pub transaction: Option<TransactionView>,
    /// Outcome of the transaction itself.
    #[serde(default)]
    pub transaction_outcome: Option<ExecutionOutcomeWithId>,
    /// Outcomes of all receipts spawned by the transaction.
    #[serde(default)]
    pub receipts_outcome: Vec<ExecutionOutcomeWithId>,
    /// Full receipt details.
    #[serde(default)]
    pub receipts: Vec<Receipt>,
}

impl FinalExecutionOutcomeWithReceipts {
    /// Check if the transaction succeeded.
    pub fn is_success(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::SuccessValue(_))
    }

    /// Check if the transaction failed.
    pub fn is_failure(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::Failure(_))
    }

    /// Get the transaction hash.
    pub fn transaction_hash(&self) -> Option<&CryptoHash> {
        self.transaction_outcome.as_ref().map(|o| &o.id)
    }
}

// ============================================================================
// Node status types
// ============================================================================

/// Node status response.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Protocol version.
    pub protocol_version: u32,
    /// Latest protocol version supported.
    pub latest_protocol_version: u32,
    /// Chain ID.
    pub chain_id: String,
    /// Genesis hash.
    pub genesis_hash: CryptoHash,
    /// RPC address.
    #[serde(default)]
    pub rpc_addr: Option<String>,
    /// Node public key.
    #[serde(default)]
    pub node_public_key: Option<String>,
    /// Node key (deprecated).
    #[serde(default)]
    pub node_key: Option<String>,
    /// Validator account ID (if validating).
    #[serde(default)]
    pub validator_account_id: Option<AccountId>,
    /// Validator public key (if validating).
    #[serde(default)]
    pub validator_public_key: Option<PublicKey>,
    /// List of current validators.
    #[serde(default)]
    pub validators: Vec<ValidatorInfo>,
    /// Sync information.
    pub sync_info: SyncInfo,
    /// Node version.
    pub version: NodeVersion,
    /// Uptime in seconds.
    #[serde(default)]
    pub uptime_sec: Option<u64>,
}

/// Validator information.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorInfo {
    /// Validator account ID.
    pub account_id: AccountId,
}

/// Sync information.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncInfo {
    /// Latest block hash.
    pub latest_block_hash: CryptoHash,
    /// Latest block height.
    pub latest_block_height: u64,
    /// Latest state root.
    #[serde(default)]
    pub latest_state_root: Option<CryptoHash>,
    /// Latest block timestamp.
    pub latest_block_time: String,
    /// Whether the node is syncing.
    pub syncing: bool,
    /// Earliest block hash (if available).
    #[serde(default)]
    pub earliest_block_hash: Option<CryptoHash>,
    /// Earliest block height (if available).
    #[serde(default)]
    pub earliest_block_height: Option<u64>,
    /// Earliest block time (if available).
    #[serde(default)]
    pub earliest_block_time: Option<String>,
    /// Current epoch ID.
    #[serde(default)]
    pub epoch_id: Option<CryptoHash>,
    /// Epoch start height.
    #[serde(default)]
    pub epoch_start_height: Option<u64>,
}

/// Node version information.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeVersion {
    /// Version string.
    pub version: String,
    /// Build string.
    pub build: String,
    /// Git commit hash.
    #[serde(default)]
    pub commit: Option<String>,
    /// Rust compiler version.
    #[serde(default)]
    pub rustc_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account_view(amount: u128, locked: u128, storage_usage: u64) -> AccountView {
        AccountView {
            amount: NearToken::from_yoctonear(amount),
            locked: NearToken::from_yoctonear(locked),
            code_hash: CryptoHash::default(),
            storage_usage,
            storage_paid_at: 0,
            global_contract_hash: None,
            global_contract_account_id: None,
            block_height: 0,
            block_hash: CryptoHash::default(),
        }
    }

    #[test]
    fn test_available_balance_no_stake_no_storage() {
        // No storage, no stake -> all balance is available
        let view = make_account_view(1_000_000_000_000_000_000_000_000, 0, 0); // 1 NEAR
        assert_eq!(view.available(), view.amount);
    }

    #[test]
    fn test_available_balance_with_storage_no_stake() {
        // 1000 bytes storage (= 0.00001 NEAR * 1000 = 0.01 NEAR = 10^22 yocto)
        // Amount: 1 NEAR = 10^24 yocto
        // Available should be: 1 NEAR - 0.01 NEAR = 0.99 NEAR
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let storage_usage = 1000u64;
        let storage_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128; // 10^22

        let view = make_account_view(amount, 0, storage_usage);
        let expected = NearToken::from_yoctonear(amount - storage_cost);
        assert_eq!(view.available(), expected);
    }

    #[test]
    fn test_available_balance_stake_covers_storage() {
        // Staked amount >= storage cost -> all liquid balance is available
        // 1000 bytes storage = 10^22 yocto cost
        // 1 NEAR staked = 10^24 yocto (more than storage cost)
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR liquid
        let locked = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR staked
        let storage_usage = 1000u64;

        let view = make_account_view(amount, locked, storage_usage);
        // All liquid balance should be available since stake covers storage
        assert_eq!(view.available(), view.amount);
    }

    #[test]
    fn test_available_balance_stake_partially_covers_storage() {
        // Staked = 0.005 NEAR = 5 * 10^21 yocto
        // Storage = 1000 bytes = 0.01 NEAR = 10^22 yocto
        // Reserved = 0.01 - 0.005 = 0.005 NEAR = 5 * 10^21 yocto
        // Amount = 1 NEAR
        // Available = 1 NEAR - 0.005 NEAR = 0.995 NEAR
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let locked = 5_000_000_000_000_000_000_000u128; // 0.005 NEAR
        let storage_usage = 1000u64;
        let storage_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128; // 10^22
        let reserved = storage_cost - locked; // 5 * 10^21

        let view = make_account_view(amount, locked, storage_usage);
        let expected = NearToken::from_yoctonear(amount - reserved);
        assert_eq!(view.available(), expected);
    }

    #[test]
    fn test_storage_cost_calculation() {
        let storage_usage = 1000u64;
        let view = make_account_view(1_000_000_000_000_000_000_000_000, 0, storage_usage);

        let expected_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128;
        assert_eq!(
            view.storage_cost(),
            NearToken::from_yoctonear(expected_cost)
        );
    }

    #[test]
    fn test_storage_cost_zero_when_stake_covers() {
        // Staked > storage cost -> storage_cost returns 0
        let locked = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let view = make_account_view(1_000_000_000_000_000_000_000_000, locked, 1000);

        assert_eq!(view.storage_cost(), NearToken::ZERO);
    }

    #[test]
    fn test_account_balance_from_view() {
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let locked = 500_000_000_000_000_000_000_000u128; // 0.5 NEAR
        let storage_usage = 1000u64;

        let view = make_account_view(amount, locked, storage_usage);
        let balance = AccountBalance::from(view.clone());

        assert_eq!(balance.total, view.amount);
        assert_eq!(balance.available, view.available());
        assert_eq!(balance.locked, view.locked);
        assert_eq!(balance.storage_cost, view.storage_cost());
        assert_eq!(balance.storage_usage, storage_usage);
    }

    // ========================================================================
    // ViewFunctionResult tests
    // ========================================================================

    fn make_view_result(result: Vec<u8>) -> ViewFunctionResult {
        ViewFunctionResult {
            result,
            logs: vec![],
            block_height: 12345,
            block_hash: CryptoHash::default(),
        }
    }

    #[test]
    fn test_view_function_result_bytes() {
        let data = vec![1, 2, 3, 4, 5];
        let result = make_view_result(data.clone());
        assert_eq!(result.bytes(), &data[..]);
    }

    #[test]
    fn test_view_function_result_as_string() {
        let result = make_view_result(b"hello world".to_vec());
        assert_eq!(result.as_string().unwrap(), "hello world");
    }

    #[test]
    fn test_view_function_result_json() {
        let result = make_view_result(b"42".to_vec());
        let value: u64 = result.json().unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_view_function_result_json_object() {
        let result = make_view_result(b"{\"count\":123}".to_vec());
        let value: serde_json::Value = result.json().unwrap();
        assert_eq!(value["count"], 123);
    }

    #[test]
    fn test_view_function_result_borsh() {
        // Borsh-encode a u64 value
        let original: u64 = 42;
        let encoded = borsh::to_vec(&original).unwrap();
        let result = make_view_result(encoded);

        let decoded: u64 = result.borsh().unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_view_function_result_borsh_struct() {
        #[derive(borsh::BorshSerialize, borsh::BorshDeserialize, PartialEq, Debug)]
        struct TestStruct {
            value: u64,
            name: String,
        }

        let original = TestStruct {
            value: 123,
            name: "test".to_string(),
        };
        let encoded = borsh::to_vec(&original).unwrap();
        let result = make_view_result(encoded);

        let decoded: TestStruct = result.borsh().unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_view_function_result_borsh_error() {
        // Invalid Borsh data for a u64 (too short)
        let result = make_view_result(vec![1, 2, 3]);
        let decoded: Result<u64, _> = result.borsh();
        assert!(decoded.is_err());
    }

    #[test]
    fn test_gas_key_function_call_deserialization() {
        let json = serde_json::json!({
            "GasKeyFunctionCall": {
                "balance": "1000000000000000000000000",
                "num_nonces": 5,
                "allowance": "500000000000000000000000",
                "receiver_id": "app.near",
                "method_names": ["call_method"]
            }
        });
        let perm: AccessKeyPermissionView = serde_json::from_value(json).unwrap();
        assert!(matches!(
            perm,
            AccessKeyPermissionView::GasKeyFunctionCall { .. }
        ));
    }

    #[test]
    fn test_gas_key_full_access_deserialization() {
        let json = serde_json::json!({
            "GasKeyFullAccess": {
                "balance": "1000000000000000000000000",
                "num_nonces": 10
            }
        });
        let perm: AccessKeyPermissionView = serde_json::from_value(json).unwrap();
        assert!(matches!(
            perm,
            AccessKeyPermissionView::GasKeyFullAccess { .. }
        ));
    }

    #[test]
    fn test_transfer_to_gas_key_action_view_deserialization() {
        let json = serde_json::json!({
            "TransferToGasKey": {
                "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                "deposit": "1000000000000000000000000"
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        assert!(matches!(action, ActionView::TransferToGasKey { .. }));
    }

    #[test]
    fn test_withdraw_from_gas_key_action_view_deserialization() {
        let json = serde_json::json!({
            "WithdrawFromGasKey": {
                "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                "amount": "500000000000000000000000"
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        assert!(matches!(action, ActionView::WithdrawFromGasKey { .. }));
    }

    // ========================================================================
    // FinalExecutionStatus tests
    // ========================================================================

    #[test]
    fn test_final_execution_status_default() {
        let status = FinalExecutionStatus::default();
        assert!(matches!(status, FinalExecutionStatus::NotStarted));
    }

    #[test]
    fn test_final_execution_status_not_started() {
        let json = serde_json::json!("NotStarted");
        let status: FinalExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, FinalExecutionStatus::NotStarted));
    }

    #[test]
    fn test_final_execution_status_started() {
        let json = serde_json::json!("Started");
        let status: FinalExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, FinalExecutionStatus::Started));
    }

    #[test]
    fn test_final_execution_status_success_value() {
        let json = serde_json::json!({"SuccessValue": "aGVsbG8="});
        let status: FinalExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, FinalExecutionStatus::SuccessValue(ref s) if s == "aGVsbG8="));
    }

    #[test]
    fn test_final_execution_status_failure() {
        let json = serde_json::json!({
            "Failure": {
                "ActionError": {
                    "index": 0,
                    "kind": {
                        "FunctionCallError": {
                            "ExecutionError": "Smart contract panicked"
                        }
                    }
                }
            }
        });
        let status: FinalExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, FinalExecutionStatus::Failure(_)));
    }

    // ========================================================================
    // FinalExecutionOutcome helper tests
    // ========================================================================

    #[test]
    fn test_final_execution_outcome_deserialization() {
        let json = serde_json::json!({
            "final_execution_status": "FINAL",
            "status": {"SuccessValue": ""},
            "transaction": {
                "signer_id": "alice.near",
                "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                "nonce": 1,
                "receiver_id": "bob.near",
                "actions": [{"Transfer": {"deposit": "1000000000000000000000000"}}],
                "signature": "ed25519:3s1dvMqNDCByoMnDnkhB4GPjTSXCRt4nt3Af5n1RX8W7aJ2FC6MfRf5BNXZ52EBifNJnNVBsGvke6GRYuaEYJXt5",
                "hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"
            },
            "transaction_outcome": {
                "id": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U",
                "outcome": {
                    "executor_id": "alice.near",
                    "gas_burnt": 223182562500_i64,
                    "tokens_burnt": "22318256250000000000",
                    "logs": [],
                    "receipt_ids": ["3GTGoiN3FEoJenSw5ob4YMmFEV2Fbiichj3FDBnM78xK"],
                    "status": {"SuccessReceiptId": "3GTGoiN3FEoJenSw5ob4YMmFEV2Fbiichj3FDBnM78xK"}
                },
                "block_hash": "A6DJpKBhmAMmBuQXtY3dWbo8dGVSQ9yH7BQSJBfn8rBo",
                "proof": []
            },
            "receipts_outcome": []
        });
        let outcome: FinalExecutionOutcome = serde_json::from_value(json).unwrap();
        assert_eq!(outcome.final_execution_status, TxExecutionStatus::Final);
        assert!(outcome.is_success());
        assert!(!outcome.is_failure());
        assert!(!outcome.is_pending());
    }

    #[test]
    fn test_final_execution_outcome_pending() {
        let json = serde_json::json!({
            "final_execution_status": "NONE"
        });
        let outcome: FinalExecutionOutcome = serde_json::from_value(json).unwrap();
        assert_eq!(outcome.final_execution_status, TxExecutionStatus::None);
        assert!(matches!(outcome.status, FinalExecutionStatus::NotStarted));
        assert!(outcome.is_pending());
        assert!(!outcome.is_success());
    }

    #[test]
    fn test_final_execution_outcome_failure() {
        let json = serde_json::json!({
            "final_execution_status": "EXECUTED_OPTIMISTIC",
            "status": {
                "Failure": {
                    "ActionError": {
                        "index": 0,
                        "kind": {
                            "FunctionCallError": {
                                "ExecutionError": "Smart contract panicked"
                            }
                        }
                    }
                }
            }
        });
        let outcome: FinalExecutionOutcome = serde_json::from_value(json).unwrap();
        assert!(outcome.is_failure());
        assert!(!outcome.is_success());
        assert!(outcome.failure_message().is_some());
        assert!(outcome.failure_error().is_some());
    }

    // ========================================================================
    // ExecutionStatus tests (per-receipt)
    // ========================================================================

    #[test]
    fn test_execution_status_unknown() {
        let json = serde_json::json!("Unknown");
        let status: ExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, ExecutionStatus::Unknown));
    }

    #[test]
    fn test_execution_status_success_value() {
        let json = serde_json::json!({"SuccessValue": "aGVsbG8="});
        let status: ExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, ExecutionStatus::SuccessValue(_)));
    }

    #[test]
    fn test_execution_status_success_receipt_id() {
        let json =
            serde_json::json!({"SuccessReceiptId": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"});
        let status: ExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, ExecutionStatus::SuccessReceiptId(_)));
    }

    // ========================================================================
    // GlobalContractIdentifierView tests
    // ========================================================================

    #[test]
    fn test_global_contract_id_view_new_format_hash() {
        let json = serde_json::json!({"hash": "9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K"});
        let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
        assert!(matches!(id, GlobalContractIdentifierView::CodeHash(_)));
    }

    #[test]
    fn test_global_contract_id_view_new_format_account() {
        let json = serde_json::json!({"account_id": "alice.near"});
        let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
        assert!(matches!(id, GlobalContractIdentifierView::AccountId(_)));
    }

    #[test]
    fn test_global_contract_id_view_deprecated_hash() {
        let json = serde_json::json!("9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K");
        let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
        assert!(matches!(id, GlobalContractIdentifierView::CodeHash(_)));
    }

    #[test]
    fn test_global_contract_id_view_deprecated_account() {
        let json = serde_json::json!("alice.near");
        let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
        assert!(matches!(id, GlobalContractIdentifierView::AccountId(_)));
    }

    // ========================================================================
    // DeterministicStateInit ActionView tests
    // ========================================================================

    #[test]
    fn test_action_view_deterministic_state_init() {
        let json = serde_json::json!({
            "DeterministicStateInit": {
                "code": {"hash": "9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K"},
                "data": {"a2V5": "dmFsdWU="},
                "deposit": "1000000000000000000000000"
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        match action {
            ActionView::DeterministicStateInit {
                code,
                data,
                deposit,
            } => {
                assert!(matches!(code, GlobalContractIdentifierView::CodeHash(_)));
                assert_eq!(data.len(), 1);
                assert_eq!(data.get("a2V5").unwrap(), "dmFsdWU=");
                assert_eq!(deposit, NearToken::from_near(1));
            }
            _ => panic!("Expected DeterministicStateInit"),
        }
    }

    #[test]
    fn test_action_view_deterministic_state_init_empty_data() {
        let json = serde_json::json!({
            "DeterministicStateInit": {
                "code": {"account_id": "publisher.near"},
                "deposit": "0"
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        match action {
            ActionView::DeterministicStateInit { code, data, .. } => {
                assert!(matches!(code, GlobalContractIdentifierView::AccountId(_)));
                assert!(data.is_empty());
            }
            _ => panic!("Expected DeterministicStateInit"),
        }
    }

    // ========================================================================
    // GlobalContractDistribution receipt tests
    // ========================================================================

    #[test]
    fn test_receipt_global_contract_distribution() {
        let json = serde_json::json!({
            "GlobalContractDistribution": {
                "id": {"hash": "9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K"},
                "target_shard": 3,
                "already_delivered_shards": [0, 1, 2],
                "code": "AGFzbQ==",
                "nonce": 42
            }
        });
        let content: ReceiptContent = serde_json::from_value(json).unwrap();
        match content {
            ReceiptContent::GlobalContractDistribution {
                id,
                target_shard,
                already_delivered_shards,
                code,
                nonce,
            } => {
                assert!(matches!(id, GlobalContractIdentifierView::CodeHash(_)));
                assert_eq!(target_shard, 3);
                assert_eq!(already_delivered_shards, vec![0, 1, 2]);
                assert_eq!(code, "AGFzbQ==");
                assert_eq!(nonce, Some(42));
            }
            _ => panic!("Expected GlobalContractDistribution"),
        }
    }

    #[test]
    fn test_receipt_global_contract_distribution_without_nonce() {
        let json = serde_json::json!({
            "GlobalContractDistribution": {
                "id": {"account_id": "publisher.near"},
                "target_shard": 0,
                "already_delivered_shards": [],
                "code": "AGFzbQ=="
            }
        });
        let content: ReceiptContent = serde_json::from_value(json).unwrap();
        match content {
            ReceiptContent::GlobalContractDistribution { nonce, .. } => {
                assert_eq!(nonce, None);
            }
            _ => panic!("Expected GlobalContractDistribution"),
        }
    }

    #[test]
    fn test_gas_profile_entry_deserialization() {
        let json = serde_json::json!({
            "cost_category": "WASM_HOST_COST",
            "cost": "BASE",
            "gas_used": "2646228750"
        });
        let entry: GasProfileEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.cost_category, "WASM_HOST_COST");
        assert_eq!(entry.cost, "BASE");
        assert_eq!(entry.gas_used, "2646228750");
    }

    #[test]
    fn test_transaction_view_with_signature() {
        let json = serde_json::json!({
            "signer_id": "alice.near",
            "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
            "nonce": 1,
            "receiver_id": "bob.near",
            "hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U",
            "actions": [{"Transfer": {"deposit": "1000000000000000000000000"}}],
            "signature": "ed25519:3s1dvMqNDCByoMnDnkhB4GPjTSXCRt4nt3Af5n1RX8W7aJ2FC6MfRf5BNXZ52EBifNJnNVBsGvke6GRYuaEYJXt5"
        });
        let tx: TransactionView = serde_json::from_value(json).unwrap();
        assert_eq!(tx.signer_id.as_str(), "alice.near");
        assert!(tx.signature.to_string().starts_with("ed25519:"));
    }
}
