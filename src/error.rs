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

/// RPC-specific errors.
#[derive(Debug, Error)]
pub enum RpcError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("RPC error: {message} (code: {code})")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },

    #[error("Account not found: {0}")]
    AccountNotFound(AccountId),

    #[error("Access key not found: {account_id} / {public_key}")]
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
    },

    #[error("Contract execution failed: {message}")]
    ContractPanic { message: String },

    #[error("Invalid nonce: expected {expected}, got {actual}")]
    InvalidNonce { expected: u64, actual: u64 },

    #[error("Timeout after {0} retries")]
    Timeout(u32),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

impl RpcError {
    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            RpcError::Http(e) => e.is_timeout() || e.is_connect(),
            RpcError::Timeout(_) => true,
            RpcError::Rpc { code, .. } => {
                // Retry on server errors
                *code == -32000 || *code == -32603
            }
            _ => false,
        }
    }
}

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
