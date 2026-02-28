//! RPC response types.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;

use super::error::TxExecutionError;
use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey};

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
    pub author: String,
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
    pub validator_proposals: Vec<ValidatorProposal>,
    /// Chunk mask (which shards have chunks).
    #[serde(default)]
    pub chunk_mask: Vec<bool>,
    /// Gas price for this block.
    pub gas_price: String,
    /// Block ordinal (may be None).
    #[serde(default)]
    pub block_ordinal: Option<u64>,
    /// Total supply of NEAR tokens.
    pub total_supply: String,
    /// Challenges result.
    #[serde(default)]
    pub challenges_result: Vec<serde_json::Value>,
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
    pub epoch_sync_data_hash: Option<String>,
    /// Block approvals (nullable signatures).
    #[serde(default)]
    pub approvals: Vec<Option<String>>,
    /// Block signature.
    pub signature: String,
    /// Latest protocol version.
    pub latest_protocol_version: u32,
}

/// Validator proposal in block header.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorProposal {
    /// Validator account ID.
    pub account_id: String,
    /// Public key.
    pub public_key: String,
    /// Stake amount.
    pub stake: String,
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
    pub validator_reward: String,
    /// Balance burnt.
    pub balance_burnt: String,
    /// Outgoing receipts root.
    pub outgoing_receipts_root: CryptoHash,
    /// Transaction root.
    pub tx_root: CryptoHash,
    /// Validator proposals.
    #[serde(default)]
    pub validator_proposals: Vec<ValidatorProposal>,
    /// Chunk signature.
    pub signature: String,
}

/// Gas price response.
#[derive(Debug, Clone, Deserialize)]
pub struct GasPrice {
    /// Gas price in yoctoNEAR.
    pub gas_price: String,
}

impl GasPrice {
    /// Get gas price as u128.
    pub fn as_u128(&self) -> u128 {
        self.gas_price.parse().unwrap_or(0)
    }
}

// ============================================================================
// Transaction outcome types
// ============================================================================

/// Transaction execution status levels.
///
/// Determines when the RPC should return a response after submitting a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FinalExecutionStatus {
    /// Don't wait - returns immediately after basic validation.
    None,
    /// Wait until transaction is included in a block.
    Included,
    /// Wait until transaction execution completes (fast, works well for sandbox/testnet).
    ExecutedOptimistic,
    /// Wait until the block containing the transaction is finalized.
    IncludedFinal,
    /// Wait until both INCLUDED_FINAL and EXECUTED_OPTIMISTIC conditions are met.
    Executed,
    /// Wait until the block with the last non-refund receipt is finalized (full finality).
    Final,
}

/// Final execution outcome from send_tx RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcome {
    /// The execution status level that was reached.
    pub final_execution_status: FinalExecutionStatus,
    /// Overall transaction status (only present for executed transactions).
    #[serde(default)]
    pub status: Option<ExecutionStatus>,
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
        matches!(
            &self.status,
            Some(ExecutionStatus::SuccessValue(_) | ExecutionStatus::SuccessReceiptId(_))
        )
    }

    /// Check if the transaction failed.
    pub fn is_failure(&self) -> bool {
        matches!(&self.status, Some(ExecutionStatus::Failure(_)))
    }

    /// Check if the transaction is still pending.
    pub fn is_pending(&self) -> bool {
        matches!(
            self.final_execution_status,
            FinalExecutionStatus::None
                | FinalExecutionStatus::Included
                | FinalExecutionStatus::IncludedFinal
        )
    }

    /// Get the success value if present (base64 decoded).
    pub fn success_value(&self) -> Option<Vec<u8>> {
        match &self.status {
            Some(ExecutionStatus::SuccessValue(s)) => STANDARD.decode(s).ok(),
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
            Some(ExecutionStatus::Failure(err)) => Some(err.to_string()),
            _ => None,
        }
    }

    /// Get the typed execution error if present.
    pub fn failure_error(&self) -> Option<&TxExecutionError> {
        match &self.status {
            Some(ExecutionStatus::Failure(err)) => Some(err),
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

/// Execution status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ExecutionStatus {
    /// Unknown status.
    Unknown,
    /// Execution is pending.
    Pending,
    /// Execution failed.
    Failure(TxExecutionError),
    /// Execution succeeded with a value (base64 encoded).
    SuccessValue(String),
    /// Execution succeeded with a receipt ID.
    SuccessReceiptId(CryptoHash),
}

/// Transaction view in outcome.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionView {
    /// Signer account.
    pub signer_id: AccountId,
    /// Signer public key.
    pub public_key: String,
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
    #[serde(default)]
    pub signature: Option<String>,
    /// Priority fee (optional, for congestion pricing).
    #[serde(default)]
    pub priority_fee: Option<u64>,
}

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
        public_key: String,
    },
    AddKey {
        public_key: String,
        access_key: serde_json::Value,
    },
    DeleteKey {
        public_key: String,
    },
    DeleteAccount {
        beneficiary_id: AccountId,
    },
    Delegate {
        delegate_action: serde_json::Value,
        signature: String,
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
        code_hash: String,
    },
    #[serde(rename = "UseGlobalContractByAccountId")]
    UseGlobalContractByAccountId {
        account_id: String,
    },
    #[serde(rename = "DeterministicStateInit")]
    DeterministicStateInit {
        deposit: NearToken,
    },
    TransferToGasKey {
        public_key: String,
        deposit: NearToken,
    },
    WithdrawFromGasKey {
        public_key: String,
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
    /// Cost name.
    #[serde(default)]
    pub cost: Option<String>,
    /// Cost category.
    #[serde(default)]
    pub cost_category: Option<String>,
    /// Gas used for this cost.
    #[serde(default)]
    pub gas_used: Option<String>,
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

/// Receipt content - either action or data.
#[derive(Debug, Clone, Deserialize)]
pub enum ReceiptContent {
    /// Action receipt.
    Action(ActionReceiptData),
    /// Data receipt.
    Data(DataReceiptData),
}

/// Action receipt data.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionReceiptData {
    /// Signer account ID.
    pub signer_id: AccountId,
    /// Signer public key.
    pub signer_public_key: String,
    /// Gas price for this receipt.
    pub gas_price: String,
    /// Output data receivers.
    #[serde(default)]
    pub output_data_receivers: Vec<serde_json::Value>,
    /// Input data IDs.
    #[serde(default)]
    pub input_data_ids: Vec<String>,
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
    pub data_id: String,
    /// Data content (optional).
    #[serde(default)]
    pub data: Option<String>,
}

/// Final execution outcome with receipts (from EXPERIMENTAL_tx_status).
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcomeWithReceipts {
    /// The execution status level that was reached.
    pub final_execution_status: FinalExecutionStatus,
    /// Overall transaction status (only present for executed transactions).
    #[serde(default)]
    pub status: Option<ExecutionStatus>,
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
        matches!(
            &self.status,
            Some(ExecutionStatus::SuccessValue(_) | ExecutionStatus::SuccessReceiptId(_))
        )
    }

    /// Check if the transaction failed.
    pub fn is_failure(&self) -> bool {
        matches!(&self.status, Some(ExecutionStatus::Failure(_)))
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
    pub validator_account_id: Option<String>,
    /// Validator public key (if validating).
    #[serde(default)]
    pub validator_public_key: Option<String>,
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
    pub account_id: String,
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
}
