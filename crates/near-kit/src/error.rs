//! Error types for near-kit.
//!
//! This module provides comprehensive error types for all near-kit operations.
//!
//! # Error Hierarchy
//!
//! - [`Error`](enum@Error) — Main error type, returned by most operations
//!   - [`RpcError`] — RPC/network errors (connectivity, account not found, etc.)
//!   - [`InvalidTxError`][crate::types::InvalidTxError] — Transaction was rejected
//!     before execution (bad nonce, insufficient balance, expired, etc.)
//!
//! Action errors (contract panics, missing keys, etc.) are **not** `Err` — the
//! transaction was accepted and executed, so the outcome is returned as
//! `Ok(outcome)` where `outcome.is_failure()` is `true`.
//!
//! # Error Handling Examples
//!
//! ## Handling Transaction Errors
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! match near.transfer("bob.testnet", "1 NEAR").await {
//!     Ok(Some(outcome)) if outcome.is_success() => {
//!         println!("Success! Hash: {}", outcome.transaction_hash());
//!     }
//!     Ok(Some(outcome)) => {
//!         // Transaction executed but an action failed — inspect the outcome
//!         println!("Action failed: {:?}, gas used: {}", outcome.failure_message(), outcome.total_gas_used());
//!     }
//!     Ok(None) => {
//!         // Transaction was included but not yet executed (when using a non-executed wait level)
//!         println!("Transaction included, not yet executed");
//!     }
//!     Err(Error::InvalidTx(e)) => {
//!         // Transaction was rejected before execution (bad nonce, insufficient balance, etc.)
//!         println!("Transaction rejected: {}", e);
//!     }
//!     Err(e) => return Err(e),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Checking Retryable Errors
//!
//! ```rust,no_run
//! use near_kit::RpcError;
//!
//! fn should_retry(err: &RpcError) -> bool {
//!     err.is_retryable()
//! }
//! ```

use thiserror::Error;

use crate::types::{AccountId, DelegateDecodeError, InvalidTxError, PublicKey};

/// Error parsing an account ID.
///
/// This is a re-export of the upstream [`near_account_id::ParseAccountError`].
pub type ParseAccountIdError = near_account_id::ParseAccountError;

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

    #[error("Invalid curve point: key bytes do not represent a valid point on the curve")]
    InvalidCurvePoint,

    #[error("Invalid scalar: secret key bytes are not a valid scalar for this curve")]
    InvalidScalar,
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

/// Error during keystore operations.
#[derive(Debug, Error)]
pub enum KeyStoreError {
    #[error("Key not found for account: {0}")]
    KeyNotFound(AccountId),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid credential format: {0}")]
    InvalidFormat(String),

    #[error("Invalid key: {0}")]
    InvalidKey(#[from] ParseKeyError),

    #[error("Path error: {0}")]
    PathError(String),

    #[error("Platform keyring error: {0}")]
    Platform(String),
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
    #[error(
        "Block not found: {0}. It may have been garbage-collected. Try an archival node for blocks older than 5 epochs."
    )]
    UnknownBlock(String),

    #[error("Chunk not found: {0}. It may have been garbage-collected. Try an archival node.")]
    UnknownChunk(String),

    #[error(
        "Epoch not found for block: {0}. The block may be invalid or too old. Try an archival node."
    )]
    UnknownEpoch(String),

    #[error("Invalid shard ID: {0}")]
    InvalidShardId(String),

    // ─── Receipt Errors ───
    #[error("Receipt not found: {0}")]
    UnknownReceipt(String),

    // ─── Transaction Errors ───
    /// Structured transaction validation error, parsed from nearcore's
    /// `TxExecutionError::InvalidTxError`. Prefer matching on
    /// [`Error::InvalidTx`] instead — this variant only appears when using
    /// the low-level `rpc().send_tx()` API directly.
    #[error("Invalid transaction: {0}")]
    InvalidTx(crate::types::InvalidTxError),

    /// Fallback when the RPC returns `INVALID_TRANSACTION` but the structured
    /// error could not be deserialized into [`InvalidTxError`][crate::types::InvalidTxError].
    #[error("Invalid transaction: {message}")]
    InvalidTransaction {
        message: String,
        details: Option<serde_json::Value>,
    },

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
            RpcError::InvalidTx(e) => e.is_retryable(),
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

    /// Create an invalid transaction error, attempting to parse structured data first.
    pub fn invalid_transaction(
        message: impl Into<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        // Try to deserialize the structured error from the data field
        if let Some(ref data) = details {
            if let Some(invalid_tx) = Self::try_parse_invalid_tx(data) {
                return RpcError::InvalidTx(invalid_tx);
            }
        }

        RpcError::InvalidTransaction {
            message: message.into(),
            details,
        }
    }

    /// Try to extract an `InvalidTxError` from the RPC error data JSON.
    ///
    /// The data field typically looks like:
    /// `{"TxExecutionError": {"InvalidTxError": {"InvalidNonce": {...}}}}`
    fn try_parse_invalid_tx(data: &serde_json::Value) -> Option<crate::types::InvalidTxError> {
        // Nested form: data.TxExecutionError.InvalidTxError
        if let Some(tx_err) = data.get("TxExecutionError") {
            if let Some(invalid_tx_value) = tx_err.get("InvalidTxError") {
                if let Ok(parsed) = serde_json::from_value(invalid_tx_value.clone()) {
                    return Some(parsed);
                }
            }
        }

        // Fallback: InvalidTxError at the top level (some RPC versions)
        if let Some(invalid_tx_value) = data.get("InvalidTxError") {
            if let Ok(parsed) = serde_json::from_value(invalid_tx_value.clone()) {
                return Some(parsed);
            }
        }

        None
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
impl RpcError {
    /// Returns true if this error indicates the account was not found.
    pub fn is_account_not_found(&self) -> bool {
        matches!(self, RpcError::AccountNotFound(_))
    }

    /// Returns true if this error indicates a contract is not deployed.
    pub fn is_contract_not_deployed(&self) -> bool {
        matches!(self, RpcError::ContractNotDeployed(_))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    // ─── Configuration ───
    #[error(
        "No signer configured. Use .credentials()/.signer() on NearBuilder, .with_signer() on the client, or .sign_with() on the transaction."
    )]
    NoSigner,

    #[error(
        "No signer account ID. Call .default_account() on NearBuilder or use a signer with an account ID."
    )]
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
    Rpc(Box<RpcError>),

    // ─── Transaction validation ───
    /// Transaction was rejected before execution. No receipt was created,
    /// no nonce was incremented, no gas was consumed.
    ///
    /// This covers validation failures from both the RPC pre-check and the
    /// runtime execution layer — the caller never needs to distinguish them.
    #[error("Invalid transaction: {0}")]
    InvalidTx(Box<InvalidTxError>),

    /// Local pre-send validation failure (e.g. empty actions, bad
    /// deserialization). Not a nearcore error.
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    // ─── Signing ───
    #[error("Signing failed: {0}")]
    Signing(#[from] SignerError),

    // ─── KeyStore ───
    #[error(transparent)]
    KeyStore(#[from] KeyStoreError),

    // ─── Serialization ───
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Borsh error: {0}")]
    Borsh(String),

    #[error("Delegate action decode error: {0}")]
    DelegateDecode(#[from] DelegateDecodeError),

    // ─── Tokens ───
    #[error("Token {token} is not available on chain {chain_id}")]
    TokenNotAvailable { token: String, chain_id: String },
}

impl From<RpcError> for Error {
    fn from(err: RpcError) -> Self {
        match err {
            // Promote structured tx validation errors to Error::InvalidTx
            RpcError::InvalidTx(e) => Error::InvalidTx(Box::new(e)),
            other => Error::Rpc(Box::new(other)),
        }
    }
}

impl Error {
    /// Returns `true` if this is an [`Error::InvalidTx`] variant.
    pub fn is_invalid_tx(&self) -> bool {
        matches!(self, Error::InvalidTx(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ParseAccountIdError tests
    // ========================================================================

    #[test]
    fn test_parse_account_id_error_display() {
        // Upstream ParseAccountError has different variants.
        // Test via parsing invalid strings.
        let err = "".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("too short"));

        let err = "a".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("too short"));

        let err = "A.near".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("invalid character"));

        let err = "bad..account".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("redundant separator"));
    }

    // ========================================================================
    // ParseAmountError tests
    // ========================================================================

    #[test]
    fn test_parse_amount_error_display() {
        assert_eq!(
            ParseAmountError::AmbiguousAmount("123".to_string()).to_string(),
            "Ambiguous amount '123'. Use explicit units like '5 NEAR' or '1000 yocto'"
        );
        assert_eq!(
            ParseAmountError::InvalidFormat("xyz".to_string()).to_string(),
            "Invalid amount format: 'xyz'"
        );
        assert_eq!(
            ParseAmountError::InvalidNumber("abc".to_string()).to_string(),
            "Invalid number in amount: 'abc'"
        );
        assert_eq!(
            ParseAmountError::Overflow.to_string(),
            "Amount overflow: value too large"
        );
    }

    // ========================================================================
    // ParseGasError tests
    // ========================================================================

    #[test]
    fn test_parse_gas_error_display() {
        assert_eq!(
            ParseGasError::InvalidFormat("xyz".to_string()).to_string(),
            "Invalid gas format: 'xyz'. Use '30 Tgas', '5 Ggas', or '1000000 gas'"
        );
        assert_eq!(
            ParseGasError::InvalidNumber("abc".to_string()).to_string(),
            "Invalid number in gas: 'abc'"
        );
        assert_eq!(
            ParseGasError::Overflow.to_string(),
            "Gas overflow: value too large"
        );
    }

    // ========================================================================
    // ParseKeyError tests
    // ========================================================================

    #[test]
    fn test_parse_key_error_display() {
        assert_eq!(
            ParseKeyError::InvalidFormat.to_string(),
            "Invalid key format: expected 'ed25519:...' or 'secp256k1:...'"
        );
        assert_eq!(
            ParseKeyError::UnknownKeyType("rsa".to_string()).to_string(),
            "Unknown key type: 'rsa'"
        );
        assert_eq!(
            ParseKeyError::InvalidBase58("invalid chars".to_string()).to_string(),
            "Invalid base58 encoding: invalid chars"
        );
        assert_eq!(
            ParseKeyError::InvalidLength {
                expected: 32,
                actual: 16
            }
            .to_string(),
            "Invalid key length: expected 32 bytes, got 16"
        );
        assert_eq!(
            ParseKeyError::InvalidCurvePoint.to_string(),
            "Invalid curve point: key bytes do not represent a valid point on the curve"
        );
    }

    // ========================================================================
    // ParseHashError tests
    // ========================================================================

    #[test]
    fn test_parse_hash_error_display() {
        assert_eq!(
            ParseHashError::InvalidBase58("bad input".to_string()).to_string(),
            "Invalid base58 encoding: bad input"
        );
        assert_eq!(
            ParseHashError::InvalidLength(16).to_string(),
            "Invalid hash length: expected 32 bytes, got 16"
        );
    }

    // ========================================================================
    // SignerError tests
    // ========================================================================

    #[test]
    fn test_signer_error_display() {
        assert_eq!(
            SignerError::InvalidSeedPhrase.to_string(),
            "Invalid seed phrase"
        );
        assert_eq!(
            SignerError::SigningFailed("hardware failure".to_string()).to_string(),
            "Signing failed: hardware failure"
        );
        assert_eq!(
            SignerError::KeyDerivationFailed("path error".to_string()).to_string(),
            "Key derivation failed: path error"
        );
    }

    // ========================================================================
    // KeyStoreError tests
    // ========================================================================

    #[test]
    fn test_keystore_error_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert_eq!(
            KeyStoreError::KeyNotFound(account_id).to_string(),
            "Key not found for account: alice.near"
        );
        assert_eq!(
            KeyStoreError::InvalidFormat("missing field".to_string()).to_string(),
            "Invalid credential format: missing field"
        );
        assert_eq!(
            KeyStoreError::PathError("bad path".to_string()).to_string(),
            "Path error: bad path"
        );
        assert_eq!(
            KeyStoreError::Platform("keyring locked".to_string()).to_string(),
            "Platform keyring error: keyring locked"
        );
    }

    // ========================================================================
    // RpcError tests
    // ========================================================================

    #[test]
    fn test_rpc_error_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();

        assert_eq!(RpcError::Timeout(3).to_string(), "Timeout after 3 retries");
        assert_eq!(
            RpcError::InvalidResponse("missing result".to_string()).to_string(),
            "Invalid response: missing result"
        );
        assert_eq!(
            RpcError::AccountNotFound(account_id.clone()).to_string(),
            "Account not found: alice.near"
        );
        assert_eq!(
            RpcError::InvalidAccount("bad-account".to_string()).to_string(),
            "Invalid account ID: bad-account"
        );
        assert_eq!(
            RpcError::ContractNotDeployed(account_id.clone()).to_string(),
            "Contract not deployed on account: alice.near"
        );
        assert_eq!(
            RpcError::ContractStateTooLarge(account_id.clone()).to_string(),
            "Contract state too large for account: alice.near"
        );
        assert_eq!(
            RpcError::UnknownBlock("12345".to_string()).to_string(),
            "Block not found: 12345. It may have been garbage-collected. Try an archival node for blocks older than 5 epochs."
        );
        assert_eq!(
            RpcError::UnknownChunk("abc123".to_string()).to_string(),
            "Chunk not found: abc123. It may have been garbage-collected. Try an archival node."
        );
        assert_eq!(
            RpcError::UnknownEpoch("epoch1".to_string()).to_string(),
            "Epoch not found for block: epoch1. The block may be invalid or too old. Try an archival node."
        );
        assert_eq!(
            RpcError::UnknownReceipt("receipt123".to_string()).to_string(),
            "Receipt not found: receipt123"
        );
        assert_eq!(
            RpcError::InvalidShardId("99".to_string()).to_string(),
            "Invalid shard ID: 99"
        );
        assert_eq!(
            RpcError::ShardUnavailable("shard 0".to_string()).to_string(),
            "Shard unavailable: shard 0"
        );
        assert_eq!(
            RpcError::NodeNotSynced("syncing...".to_string()).to_string(),
            "Node not synced: syncing..."
        );
        assert_eq!(
            RpcError::InternalError("database error".to_string()).to_string(),
            "Internal server error: database error"
        );
        assert_eq!(
            RpcError::ParseError("invalid json".to_string()).to_string(),
            "Parse error: invalid json"
        );
    }

    #[test]
    fn test_rpc_error_is_retryable() {
        use crate::types::InvalidTxError;

        // Retryable errors
        assert!(RpcError::Timeout(3).is_retryable());
        assert!(RpcError::ShardUnavailable("shard 0".to_string()).is_retryable());
        assert!(RpcError::NodeNotSynced("syncing".to_string()).is_retryable());
        assert!(RpcError::InternalError("db error".to_string()).is_retryable());
        assert!(
            RpcError::RequestTimeout {
                message: "timeout".to_string(),
                transaction_hash: None,
            }
            .is_retryable()
        );
        assert!(
            RpcError::InvalidTx(InvalidTxError::InvalidNonce {
                tx_nonce: 5,
                ak_nonce: 10
            })
            .is_retryable()
        );
        assert!(
            RpcError::InvalidTx(InvalidTxError::ShardCongested {
                congestion_level: 1.0,
                shard_id: 0,
            })
            .is_retryable()
        );
        assert!(
            RpcError::Network {
                message: "connection reset".to_string(),
                status_code: Some(503),
                retryable: true,
            }
            .is_retryable()
        );
        assert!(
            RpcError::Rpc {
                code: -32000,
                message: "server error".to_string(),
                data: None,
            }
            .is_retryable()
        );

        // Non-retryable errors
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert!(!RpcError::AccountNotFound(account_id.clone()).is_retryable());
        assert!(!RpcError::ContractNotDeployed(account_id.clone()).is_retryable());
        assert!(!RpcError::InvalidAccount("bad".to_string()).is_retryable());
        assert!(!RpcError::UnknownBlock("12345".to_string()).is_retryable());
        assert!(!RpcError::ParseError("bad json".to_string()).is_retryable());
        assert!(
            !RpcError::InvalidTx(InvalidTxError::NotEnoughBalance {
                signer_id: account_id.clone(),
                balance: crate::types::NearToken::from_near(1),
                cost: crate::types::NearToken::from_near(100),
            })
            .is_retryable()
        );
        assert!(
            !RpcError::InvalidTransaction {
                message: "invalid".to_string(),
                details: None,
            }
            .is_retryable()
        );
    }

    #[test]
    fn test_rpc_error_network_constructor() {
        let err = RpcError::network("connection refused", Some(503), true);
        match err {
            RpcError::Network {
                message,
                status_code,
                retryable,
            } => {
                assert_eq!(message, "connection refused");
                assert_eq!(status_code, Some(503));
                assert!(retryable);
            }
            _ => panic!("Expected Network error"),
        }
    }

    #[test]
    fn test_rpc_error_invalid_transaction_constructor_unstructured() {
        let err = RpcError::invalid_transaction("invalid nonce", None);
        match err {
            RpcError::InvalidTransaction { message, details } => {
                assert_eq!(message, "invalid nonce");
                assert!(details.is_none());
            }
            _ => panic!("Expected InvalidTransaction error"),
        }
    }

    #[test]
    fn test_rpc_error_invalid_transaction_constructor_structured() {
        // When data contains a parseable InvalidTxError, it should produce InvalidTx
        let data = serde_json::json!({
            "TxExecutionError": {
                "InvalidTxError": {
                    "InvalidNonce": {
                        "tx_nonce": 5,
                        "ak_nonce": 10
                    }
                }
            }
        });
        let err = RpcError::invalid_transaction("invalid nonce", Some(data));
        match err {
            RpcError::InvalidTx(crate::types::InvalidTxError::InvalidNonce {
                tx_nonce,
                ak_nonce,
            }) => {
                assert_eq!(tx_nonce, 5);
                assert_eq!(ak_nonce, 10);
            }
            other => panic!("Expected InvalidTx(InvalidNonce), got: {other:?}"),
        }
    }

    #[test]
    fn test_rpc_error_function_call_constructor() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::function_call(
            account_id.clone(),
            "my_method",
            Some("assertion failed".to_string()),
            vec!["log1".to_string(), "log2".to_string()],
        );
        match err {
            RpcError::FunctionCall {
                contract_id,
                method_name,
                panic,
                logs,
            } => {
                assert_eq!(contract_id, account_id);
                assert_eq!(method_name, "my_method");
                assert_eq!(panic, Some("assertion failed".to_string()));
                assert_eq!(logs, vec!["log1", "log2"]);
            }
            _ => panic!("Expected FunctionCall error"),
        }
    }

    #[test]
    fn test_rpc_error_is_account_not_found() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert!(RpcError::AccountNotFound(account_id).is_account_not_found());
        assert!(!RpcError::Timeout(3).is_account_not_found());
    }

    #[test]
    fn test_rpc_error_is_contract_not_deployed() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert!(RpcError::ContractNotDeployed(account_id).is_contract_not_deployed());
        assert!(!RpcError::Timeout(3).is_contract_not_deployed());
    }

    #[test]
    fn test_rpc_error_contract_execution_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::ContractExecution {
            contract_id: account_id,
            method_name: Some("my_method".to_string()),
            message: "execution failed".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Contract execution failed on contract.near: execution failed"
        );
    }

    #[test]
    fn test_rpc_error_function_call_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::FunctionCall {
            contract_id: account_id.clone(),
            method_name: "my_method".to_string(),
            panic: Some("assertion failed".to_string()),
            logs: vec![],
        };
        assert_eq!(
            err.to_string(),
            "Function call error on contract.near.my_method: assertion failed"
        );

        let err_no_panic = RpcError::FunctionCall {
            contract_id: account_id,
            method_name: "other_method".to_string(),
            panic: None,
            logs: vec![],
        };
        assert_eq!(
            err_no_panic.to_string(),
            "Function call error on contract.near.other_method: unknown error"
        );
    }

    #[test]
    fn test_rpc_error_invalid_tx_display() {
        use crate::types::InvalidTxError;
        let err = RpcError::InvalidTx(InvalidTxError::InvalidNonce {
            tx_nonce: 5,
            ak_nonce: 10,
        });
        assert!(err.to_string().contains("invalid nonce"));
    }

    #[test]
    fn test_rpc_error_access_key_not_found_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let err = RpcError::AccessKeyNotFound {
            account_id,
            public_key: public_key.clone(),
        };
        assert!(err.to_string().contains("alice.near"));
        assert!(err.to_string().contains(&public_key.to_string()));
    }

    #[test]
    fn test_rpc_error_request_timeout_display() {
        let err = RpcError::RequestTimeout {
            message: "request timed out".to_string(),
            transaction_hash: Some("abc123".to_string()),
        };
        assert_eq!(err.to_string(), "Request timeout: request timed out");
    }

    // ========================================================================
    // Error (main type) tests
    // ========================================================================

    #[test]
    fn test_error_no_signer_display() {
        assert_eq!(
            Error::NoSigner.to_string(),
            "No signer configured. Use .credentials()/.signer() on NearBuilder, .with_signer() on the client, or .sign_with() on the transaction."
        );
    }

    #[test]
    fn test_error_no_signer_account_display() {
        assert_eq!(
            Error::NoSignerAccount.to_string(),
            "No signer account ID. Call .default_account() on NearBuilder or use a signer with an account ID."
        );
    }

    #[test]
    fn test_error_config_display() {
        assert_eq!(
            Error::Config("invalid url".to_string()).to_string(),
            "Invalid configuration: invalid url"
        );
    }

    #[test]
    fn test_error_invalid_tx_display() {
        use crate::types::InvalidTxError;
        let err = Error::InvalidTx(Box::new(InvalidTxError::Expired));
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn test_error_from_rpc_invalid_tx_promotes() {
        use crate::types::InvalidTxError;
        // RpcError::InvalidTx should become Error::InvalidTx, not Error::Rpc
        let rpc_err = RpcError::InvalidTx(InvalidTxError::InvalidNonce {
            tx_nonce: 5,
            ak_nonce: 10,
        });
        let err: Error = rpc_err.into();
        assert!(matches!(
            err,
            Error::InvalidTx(e) if matches!(*e, InvalidTxError::InvalidNonce { .. })
        ));
    }

    #[test]
    fn test_error_borsh_display() {
        assert_eq!(
            Error::Borsh("deserialization failed".to_string()).to_string(),
            "Borsh error: deserialization failed"
        );
    }

    #[test]
    fn test_error_from_parse_errors() {
        // ParseAccountIdError -> Error
        let parse_err = "".parse::<AccountId>().unwrap_err();
        let err: Error = parse_err.into();
        assert!(matches!(err, Error::ParseAccountId(_)));

        // ParseAmountError -> Error
        let parse_err = ParseAmountError::Overflow;
        let err: Error = parse_err.into();
        assert!(matches!(err, Error::ParseAmount(_)));

        // ParseGasError -> Error
        let parse_err = ParseGasError::Overflow;
        let err: Error = parse_err.into();
        assert!(matches!(err, Error::ParseGas(_)));

        // ParseKeyError -> Error
        let parse_err = ParseKeyError::InvalidFormat;
        let err: Error = parse_err.into();
        assert!(matches!(err, Error::ParseKey(_)));
    }

    #[test]
    fn test_error_from_rpc_error() {
        let rpc_err = RpcError::Timeout(3);
        let err: Error = rpc_err.into();
        assert!(matches!(err, Error::Rpc(_)));
    }

    #[test]
    fn test_error_from_signer_error() {
        let signer_err = SignerError::InvalidSeedPhrase;
        let err: Error = signer_err.into();
        assert!(matches!(err, Error::Signing(_)));
    }
}
