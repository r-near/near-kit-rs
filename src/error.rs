//! Error types for near-kit.

use thiserror::Error;

use crate::types::{AccountId, PublicKey};

/// Error parsing an account ID.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseAccountIdError {
    #[error("Account ID is empty")]
    Empty,

    #[error("Account ID '{0}' is too long (max 64 characters)")]
    TooLong(String),

    #[error("Account ID '{0}' is too short (min 2 characters for named accounts)")]
    TooShort(String),

    #[error("Account ID '{0}' contains invalid character '{1}'")]
    InvalidChar(String, char),

    #[error("Account ID '{0}' has invalid format")]
    InvalidFormat(String),
}

/// Error parsing a NEAR token amount.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseAmountError {
    #[error("Ambiguous amount '{0}'. Use explicit units like '5 NEAR' or '1000 yocto'")]
    AmbiguousAmount(String),

    #[error("Invalid amount format: '{0}'")]
    InvalidFormat(String),

    #[error("Invalid number in amount: '{0}'")]
    InvalidNumber(String),

    #[error("Amount overflow: value too large")]
    Overflow,
}

/// Error parsing a gas value.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseGasError {
    #[error("Invalid gas format: '{0}'. Use '30 Tgas', '5 Ggas', or '1000000 gas'")]
    InvalidFormat(String),

    #[error("Invalid number in gas: '{0}'")]
    InvalidNumber(String),

    #[error("Gas overflow: value too large")]
    Overflow,
}

/// Error parsing a public or secret key.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseKeyError {
    #[error("Invalid key format: expected 'ed25519:...' or 'secp256k1:...'")]
    InvalidFormat,

    #[error("Unknown key type: '{0}'")]
    UnknownKeyType(String),

    #[error("Invalid base58 encoding: {0}")]
    InvalidBase58(String),

    #[error("Invalid key length: expected {expected} bytes, got {actual}")]
    InvalidLength { expected: usize, actual: usize },
}

/// Error parsing a crypto hash.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseHashError {
    #[error("Invalid base58 encoding: {0}")]
    InvalidBase58(String),

    #[error("Invalid hash length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
}

/// Error during signing operations.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SignerError {
    #[error("Invalid seed phrase")]
    InvalidSeedPhrase,

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),
}

// ============================================================================
// RPC Errors
// ============================================================================

/// RPC-specific errors.
#[derive(Debug, Error)]
pub enum RpcError {
    // ─── Network/Transport ───
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Network error: {message}")]
    Network {
        message: String,
        status_code: Option<u16>,
        retryable: bool,
    },

    #[error("Timeout after {0} retries")]
    Timeout(u32),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    // ─── Generic RPC Error ───
    #[error("RPC error: {message} (code: {code})")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },

    // ─── Account Errors ───
    #[error("Account not found: {0}")]
    AccountNotFound(AccountId),

    #[error("Invalid account ID: {0}")]
    InvalidAccount(String),

    #[error("Access key not found: {account_id} / {public_key}")]
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
    },

    // ─── Contract Errors ───
    #[error("Contract not deployed on account: {0}")]
    ContractNotDeployed(AccountId),

    #[error("Contract state too large for account: {0}")]
    ContractStateTooLarge(AccountId),

    #[error("Contract execution failed on {contract_id}: {message}")]
    ContractExecution {
        contract_id: AccountId,
        method_name: Option<String>,
        message: String,
    },

    #[error("Contract panic: {message}")]
    ContractPanic { message: String },

    #[error("Function call error on {contract_id}.{method_name}: {}", panic.as_deref().unwrap_or("unknown error"))]
    FunctionCall {
        contract_id: AccountId,
        method_name: String,
        panic: Option<String>,
        logs: Vec<String>,
    },

    // ─── Block/Chunk Errors ───
    #[error("Block not found: {0}. It may have been garbage-collected. Try an archival node for blocks older than 5 epochs.")]
    UnknownBlock(String),

    #[error("Chunk not found: {0}. It may have been garbage-collected. Try an archival node.")]
    UnknownChunk(String),

    #[error("Epoch not found for block: {0}. The block may be invalid or too old. Try an archival node.")]
    UnknownEpoch(String),

    #[error("Invalid shard ID: {0}")]
    InvalidShardId(String),

    // ─── Receipt Errors ───
    #[error("Receipt not found: {0}")]
    UnknownReceipt(String),

    // ─── Transaction Errors ───
    #[error("Invalid transaction: {message}")]
    InvalidTransaction {
        message: String,
        details: Option<serde_json::Value>,
        shard_congested: bool,
        shard_stuck: bool,
    },

    #[error("Invalid nonce: transaction nonce {tx_nonce} must be greater than access key nonce {ak_nonce}")]
    InvalidNonce { tx_nonce: u64, ak_nonce: u64 },

    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: String, available: String },

    #[error("Gas limit exceeded: used {gas_used}, limit {gas_limit}")]
    GasLimitExceeded { gas_used: String, gas_limit: String },

    // ─── Node Errors ───
    #[error("Shard unavailable: {0}")]
    ShardUnavailable(String),

    #[error("Node not synced: {0}")]
    NodeNotSynced(String),

    #[error("Internal server error: {0}")]
    InternalError(String),

    // ─── Request Errors ───
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Request timeout: {message}")]
    RequestTimeout {
        message: String,
        transaction_hash: Option<String>,
    },
}

impl RpcError {
    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            RpcError::Http(e) => e.is_timeout() || e.is_connect(),
            RpcError::Timeout(_) => true,
            RpcError::Network { retryable, .. } => *retryable,
            RpcError::ShardUnavailable(_) => true,
            RpcError::NodeNotSynced(_) => true,
            RpcError::InternalError(_) => true,
            RpcError::RequestTimeout { .. } => true,
            RpcError::InvalidNonce { .. } => true,
            RpcError::InvalidTransaction {
                shard_congested,
                shard_stuck,
                ..
            } => *shard_congested || *shard_stuck,
            RpcError::Rpc { code, .. } => {
                // Retry on server errors
                *code == -32000 || *code == -32603
            }
            _ => false,
        }
    }

    /// Create a network error.
    pub fn network(message: impl Into<String>, status_code: Option<u16>, retryable: bool) -> Self {
        RpcError::Network {
            message: message.into(),
            status_code,
            retryable,
        }
    }

    /// Create an invalid transaction error.
    pub fn invalid_transaction(
        message: impl Into<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        let details_obj = details.as_ref();
        let shard_congested = details_obj
            .and_then(|d| d.get("ShardCongested"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let shard_stuck = details_obj
            .and_then(|d| d.get("ShardStuck"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        RpcError::InvalidTransaction {
            message: message.into(),
            details,
            shard_congested,
            shard_stuck,
        }
    }

    /// Create a function call error.
    pub fn function_call(
        contract_id: AccountId,
        method_name: impl Into<String>,
        panic: Option<String>,
        logs: Vec<String>,
    ) -> Self {
        RpcError::FunctionCall {
            contract_id,
            method_name: method_name.into(),
            panic,
            logs,
        }
    }
}

// ============================================================================
// Main Error Type
// ============================================================================

/// Main error type for near-kit operations.
#[derive(Debug, Error)]
pub enum Error {
    // ─── Configuration ───
    #[error(
        "No signer configured. Call .signer() on NearBuilder or .sign_with() on the operation."
    )]
    NoSigner,

    #[error("No signer account ID. Call .default_account() on NearBuilder or use a signer with an account ID.")]
    NoSignerAccount,

    #[error("Invalid configuration: {0}")]
    Config(String),

    // ─── Parsing ───
    #[error(transparent)]
    ParseAccountId(#[from] ParseAccountIdError),

    #[error(transparent)]
    ParseAmount(#[from] ParseAmountError),

    #[error(transparent)]
    ParseGas(#[from] ParseGasError),

    #[error(transparent)]
    ParseKey(#[from] ParseKeyError),

    // ─── RPC ───
    #[error(transparent)]
    Rpc(#[from] RpcError),

    // ─── Transaction ───
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Contract panic: {0}")]
    ContractPanic(String),

    // ─── Signing ───
    #[error("Signing failed: {0}")]
    Signing(#[from] SignerError),

    // ─── Serialization ───
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Borsh error: {0}")]
    Borsh(String),
}
