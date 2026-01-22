//! RPC response types.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;

use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey};

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
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

impl AccountView {
    /// Get available (unlocked) balance.
    pub fn available(&self) -> NearToken {
        self.amount.saturating_sub(self.locked)
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
    /// Available balance (not locked).
    pub available: NearToken,
    /// Locked balance (staked).
    pub locked: NearToken,
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
    pub access_key: AccessKeyView,
}

// ============================================================================
// Block types
// ============================================================================

/// Block information from block RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockView {
    /// Block header.
    pub header: BlockHeaderView,
    /// List of chunks in the block.
    pub chunks: Vec<ChunkHeaderView>,
}

/// Block header.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeaderView {
    /// Block height.
    pub height: u64,
    /// Block hash.
    pub hash: CryptoHash,
    /// Previous block hash.
    pub prev_hash: CryptoHash,
    /// Timestamp in nanoseconds.
    pub timestamp_nanosec: String,
    /// Epoch ID.
    pub epoch_id: CryptoHash,
    /// Next epoch ID.
    pub next_epoch_id: CryptoHash,
    /// Gas price for this block.
    pub gas_price: String,
    /// Total supply of NEAR tokens.
    pub total_supply: String,
}

/// Chunk header.
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkHeaderView {
    /// Chunk hash.
    pub chunk_hash: CryptoHash,
    /// Shard ID.
    pub shard_id: u64,
    /// Gas used in this chunk.
    pub gas_used: u64,
    /// Gas limit for this chunk.
    pub gas_limit: u64,
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

/// Final execution outcome from send_tx RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcome {
    /// Overall transaction status.
    pub status: ExecutionStatus,
    /// The transaction that was executed.
    pub transaction: TransactionView,
    /// Outcome of the transaction itself.
    pub transaction_outcome: ExecutionOutcomeWithId,
    /// Outcomes of all receipts spawned by the transaction.
    pub receipts_outcome: Vec<ExecutionOutcomeWithId>,
}

impl FinalExecutionOutcome {
    /// Check if the transaction succeeded.
    pub fn is_success(&self) -> bool {
        matches!(
            self.status,
            ExecutionStatus::SuccessValue(_) | ExecutionStatus::SuccessReceiptId(_)
        )
    }

    /// Check if the transaction failed.
    pub fn is_failure(&self) -> bool {
        matches!(self.status, ExecutionStatus::Failure(_))
    }

    /// Get the success value if present (base64 decoded).
    pub fn success_value(&self) -> Option<Vec<u8>> {
        match &self.status {
            ExecutionStatus::SuccessValue(s) => STANDARD.decode(s).ok(),
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
            ExecutionStatus::Failure(err) => Some(format!("{:?}", err)),
            _ => None,
        }
    }

    /// Get the transaction hash.
    pub fn transaction_hash(&self) -> &CryptoHash {
        &self.transaction_outcome.id
    }

    /// Get total gas used across all receipts.
    pub fn total_gas_used(&self) -> Gas {
        let tx_gas = self.transaction_outcome.outcome.gas_burnt;
        let receipt_gas: u64 = self
            .receipts_outcome
            .iter()
            .map(|r| r.outcome.gas_burnt.as_gas())
            .sum();
        Gas::from_gas(tx_gas.as_gas() + receipt_gas)
    }
}

/// Execution status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ExecutionStatus {
    /// Unknown status.
    Unknown,
    /// Execution failed.
    Failure(serde_json::Value),
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
    pub actions: Vec<ActionView>,
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
}

/// Execution outcome with ID.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionOutcomeWithId {
    /// Receipt or transaction ID.
    pub id: CryptoHash,
    /// Outcome details.
    pub outcome: ExecutionOutcome,
    /// Proof of execution (if requested).
    pub proof: Option<Vec<serde_json::Value>>,
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
    /// Get the result as a string.
    pub fn as_string(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.result.clone())
    }

    /// Deserialize the result as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.result)
    }
}

/// Node status response.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Protocol version.
    pub protocol_version: u32,
    /// Chain ID.
    pub chain_id: String,
    /// Latest block height.
    pub sync_info: SyncInfo,
    /// Node version.
    pub version: NodeVersion,
}

/// Sync information.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncInfo {
    /// Latest block hash.
    pub latest_block_hash: CryptoHash,
    /// Latest block height.
    pub latest_block_height: u64,
    /// Latest block timestamp.
    pub latest_block_time: String,
    /// Whether the node is syncing.
    pub syncing: bool,
}

/// Node version information.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeVersion {
    /// Version string.
    pub version: String,
    /// Build string.
    pub build: String,
}
