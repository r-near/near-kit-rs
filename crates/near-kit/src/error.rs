//! Error types for near-kit.

use thiserror::Error;

use crate::types::{AccountId, DelegateDecodeError, PublicKey};

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
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ParseAccountIdError Tests
    // ========================================================================

    #[test]
    fn test_parse_account_id_error_empty_display() {
        let err = ParseAccountIdError::Empty;
        assert_eq!(err.to_string(), "Account ID is empty");
    }

    #[test]
    fn test_parse_account_id_error_too_long_display() {
        let err = ParseAccountIdError::TooLong("a".repeat(65));
        assert!(err.to_string().contains("too long"));
        assert!(err.to_string().contains("max 64 characters"));
    }

    #[test]
    fn test_parse_account_id_error_too_short_display() {
        let err = ParseAccountIdError::TooShort("a".to_string());
        assert!(err.to_string().contains("too short"));
        assert!(err.to_string().contains("min 2 characters"));
    }

    #[test]
    fn test_parse_account_id_error_invalid_char_display() {
        let err = ParseAccountIdError::InvalidChar("test@account".to_string(), '@');
        assert!(err.to_string().contains("invalid character"));
        assert!(err.to_string().contains("@"));
    }

    #[test]
    fn test_parse_account_id_error_invalid_format_display() {
        let err = ParseAccountIdError::InvalidFormat("..invalid".to_string());
        assert!(err.to_string().contains("invalid format"));
    }

    #[test]
    fn test_parse_account_id_error_equality() {
        assert_eq!(ParseAccountIdError::Empty, ParseAccountIdError::Empty);
        assert_ne!(
            ParseAccountIdError::Empty,
            ParseAccountIdError::TooShort("a".to_string())
        );
    }

    // ========================================================================
    // ParseAmountError Tests
    // ========================================================================

    #[test]
    fn test_parse_amount_error_ambiguous_display() {
        let err = ParseAmountError::AmbiguousAmount("100".to_string());
        assert!(err.to_string().contains("Ambiguous"));
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("NEAR"));
        assert!(err.to_string().contains("yocto"));
    }

    #[test]
    fn test_parse_amount_error_invalid_format_display() {
        let err = ParseAmountError::InvalidFormat("not-a-number".to_string());
        assert!(err.to_string().contains("Invalid amount format"));
    }

    #[test]
    fn test_parse_amount_error_invalid_number_display() {
        let err = ParseAmountError::InvalidNumber("abc".to_string());
        assert!(err.to_string().contains("Invalid number"));
    }

    #[test]
    fn test_parse_amount_error_overflow_display() {
        let err = ParseAmountError::Overflow;
        assert!(err.to_string().contains("overflow"));
    }

    #[test]
    fn test_parse_amount_error_equality() {
        assert_eq!(ParseAmountError::Overflow, ParseAmountError::Overflow);
        assert_ne!(
            ParseAmountError::Overflow,
            ParseAmountError::InvalidFormat("x".to_string())
        );
    }

    // ========================================================================
    // ParseGasError Tests
    // ========================================================================

    #[test]
    fn test_parse_gas_error_invalid_format_display() {
        let err = ParseGasError::InvalidFormat("bad".to_string());
        assert!(err.to_string().contains("Invalid gas format"));
        assert!(err.to_string().contains("Tgas"));
    }

    #[test]
    fn test_parse_gas_error_invalid_number_display() {
        let err = ParseGasError::InvalidNumber("xyz".to_string());
        assert!(err.to_string().contains("Invalid number"));
    }

    #[test]
    fn test_parse_gas_error_overflow_display() {
        let err = ParseGasError::Overflow;
        assert!(err.to_string().contains("overflow"));
    }

    // ========================================================================
    // ParseKeyError Tests
    // ========================================================================

    #[test]
    fn test_parse_key_error_invalid_format_display() {
        let err = ParseKeyError::InvalidFormat;
        assert!(err.to_string().contains("ed25519"));
        assert!(err.to_string().contains("secp256k1"));
    }

    #[test]
    fn test_parse_key_error_unknown_key_type_display() {
        let err = ParseKeyError::UnknownKeyType("rsa".to_string());
        assert!(err.to_string().contains("Unknown key type"));
        assert!(err.to_string().contains("rsa"));
    }

    #[test]
    fn test_parse_key_error_invalid_base58_display() {
        let err = ParseKeyError::InvalidBase58("invalid chars".to_string());
        assert!(err.to_string().contains("Invalid base58"));
    }

    #[test]
    fn test_parse_key_error_invalid_length_display() {
        let err = ParseKeyError::InvalidLength {
            expected: 32,
            actual: 16,
        };
        assert!(err.to_string().contains("32"));
        assert!(err.to_string().contains("16"));
    }

    // ========================================================================
    // ParseHashError Tests
    // ========================================================================

    #[test]
    fn test_parse_hash_error_invalid_base58_display() {
        let err = ParseHashError::InvalidBase58("bad encoding".to_string());
        assert!(err.to_string().contains("Invalid base58"));
    }

    #[test]
    fn test_parse_hash_error_invalid_length_display() {
        let err = ParseHashError::InvalidLength(16);
        assert!(err.to_string().contains("32 bytes"));
        assert!(err.to_string().contains("16"));
    }

    // ========================================================================
    // SignerError Tests
    // ========================================================================

    #[test]
    fn test_signer_error_invalid_seed_phrase_display() {
        let err = SignerError::InvalidSeedPhrase;
        assert!(err.to_string().contains("Invalid seed phrase"));
    }

    #[test]
    fn test_signer_error_signing_failed_display() {
        let err = SignerError::SigningFailed("hardware wallet error".to_string());
        assert!(err.to_string().contains("Signing failed"));
        assert!(err.to_string().contains("hardware wallet error"));
    }

    #[test]
    fn test_signer_error_key_derivation_failed_display() {
        let err = SignerError::KeyDerivationFailed("invalid path".to_string());
        assert!(err.to_string().contains("Key derivation failed"));
    }

    // ========================================================================
    // KeyStoreError Tests
    // ========================================================================

    #[test]
    fn test_keystore_error_key_not_found_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        let err = KeyStoreError::KeyNotFound(account_id);
        assert!(err.to_string().contains("Key not found"));
        assert!(err.to_string().contains("alice.near"));
    }

    #[test]
    fn test_keystore_error_invalid_format_display() {
        let err = KeyStoreError::InvalidFormat("missing private_key field".to_string());
        assert!(err.to_string().contains("Invalid credential format"));
    }

    #[test]
    fn test_keystore_error_path_error_display() {
        let err = KeyStoreError::PathError("home directory not found".to_string());
        assert!(err.to_string().contains("Path error"));
    }

    #[test]
    fn test_keystore_error_platform_display() {
        let err = KeyStoreError::Platform("keychain locked".to_string());
        assert!(err.to_string().contains("Platform keyring error"));
    }

    #[test]
    fn test_keystore_error_from_parse_key_error() {
        let key_err = ParseKeyError::InvalidFormat;
        let err: KeyStoreError = key_err.into();
        assert!(err.to_string().contains("Invalid key"));
    }

    // ========================================================================
    // RpcError Tests
    // ========================================================================

    #[test]
    fn test_rpc_error_timeout_is_retryable() {
        let err = RpcError::Timeout(3);
        assert!(err.is_retryable());
        assert!(err.to_string().contains("Timeout"));
        assert!(err.to_string().contains("3"));
    }

    #[test]
    fn test_rpc_error_network_retryable() {
        let err = RpcError::network("connection refused", Some(503), true);
        assert!(err.is_retryable());
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn test_rpc_error_network_not_retryable() {
        let err = RpcError::network("bad request", Some(400), false);
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_rpc_error_shard_unavailable_is_retryable() {
        let err = RpcError::ShardUnavailable("shard 0".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_node_not_synced_is_retryable() {
        let err = RpcError::NodeNotSynced("catching up".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_internal_error_is_retryable() {
        let err = RpcError::InternalError("database error".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_request_timeout_is_retryable() {
        let err = RpcError::RequestTimeout {
            message: "took too long".to_string(),
            transaction_hash: Some("abc123".to_string()),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_invalid_nonce_is_retryable() {
        let err = RpcError::InvalidNonce {
            tx_nonce: 5,
            ak_nonce: 10,
        };
        assert!(err.is_retryable());
        assert!(err.to_string().contains("5"));
        assert!(err.to_string().contains("10"));
    }

    #[test]
    fn test_rpc_error_rpc_retryable_codes() {
        // -32000 is retryable (server error)
        let err = RpcError::Rpc {
            code: -32000,
            message: "server overloaded".to_string(),
            data: None,
        };
        assert!(err.is_retryable());

        // -32603 is retryable (internal error)
        let err = RpcError::Rpc {
            code: -32603,
            message: "internal error".to_string(),
            data: None,
        };
        assert!(err.is_retryable());

        // Other codes are not retryable
        let err = RpcError::Rpc {
            code: -32600,
            message: "invalid request".to_string(),
            data: None,
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_rpc_error_invalid_transaction_shard_congested_is_retryable() {
        let details = serde_json::json!({ "ShardCongested": true });
        let err = RpcError::invalid_transaction("shard congested", Some(details));
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_invalid_transaction_shard_stuck_is_retryable() {
        let details = serde_json::json!({ "ShardStuck": true });
        let err = RpcError::invalid_transaction("shard stuck", Some(details));
        assert!(err.is_retryable());
    }

    #[test]
    fn test_rpc_error_invalid_transaction_normal_not_retryable() {
        let err = RpcError::invalid_transaction("invalid signature", None);
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_rpc_error_account_not_found_not_retryable() {
        let account_id: AccountId = "missing.near".parse().unwrap();
        let err = RpcError::AccountNotFound(account_id);
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("missing.near"));
    }

    #[test]
    fn test_rpc_error_access_key_not_found_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let err = RpcError::AccessKeyNotFound {
            account_id,
            public_key,
        };
        assert!(err.to_string().contains("alice.near"));
        assert!(err.to_string().contains("Access key not found"));
    }

    #[test]
    fn test_rpc_error_contract_not_deployed_display() {
        let account_id: AccountId = "user.near".parse().unwrap();
        let err = RpcError::ContractNotDeployed(account_id);
        assert!(err.to_string().contains("Contract not deployed"));
        assert!(err.to_string().contains("user.near"));
    }

    #[test]
    fn test_rpc_error_contract_state_too_large_display() {
        let account_id: AccountId = "big-contract.near".parse().unwrap();
        let err = RpcError::ContractStateTooLarge(account_id);
        assert!(err.to_string().contains("state too large"));
    }

    #[test]
    fn test_rpc_error_contract_execution_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::ContractExecution {
            contract_id: account_id,
            method_name: Some("do_something".to_string()),
            message: "assertion failed".to_string(),
        };
        assert!(err.to_string().contains("contract.near"));
        assert!(err.to_string().contains("assertion failed"));
    }

    #[test]
    fn test_rpc_error_contract_panic_display() {
        let err = RpcError::ContractPanic {
            message: "index out of bounds".to_string(),
        };
        assert!(err.to_string().contains("Contract panic"));
        assert!(err.to_string().contains("index out of bounds"));
    }

    #[test]
    fn test_rpc_error_function_call_constructor() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::function_call(
            account_id,
            "my_method",
            Some("panic message".to_string()),
            vec!["log1".to_string(), "log2".to_string()],
        );

        if let RpcError::FunctionCall {
            contract_id,
            method_name,
            panic,
            logs,
        } = err
        {
            assert_eq!(contract_id.as_str(), "contract.near");
            assert_eq!(method_name, "my_method");
            assert_eq!(panic, Some("panic message".to_string()));
            assert_eq!(logs.len(), 2);
        } else {
            panic!("Expected FunctionCall variant");
        }
    }

    #[test]
    fn test_rpc_error_function_call_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::FunctionCall {
            contract_id: account_id,
            method_name: "transfer".to_string(),
            panic: Some("insufficient funds".to_string()),
            logs: vec![],
        };
        assert!(err.to_string().contains("contract.near"));
        assert!(err.to_string().contains("transfer"));
        assert!(err.to_string().contains("insufficient funds"));
    }

    #[test]
    fn test_rpc_error_function_call_no_panic_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::FunctionCall {
            contract_id: account_id,
            method_name: "transfer".to_string(),
            panic: None,
            logs: vec![],
        };
        assert!(err.to_string().contains("unknown error"));
    }

    #[test]
    fn test_rpc_error_unknown_block_display() {
        let err = RpcError::UnknownBlock("height 12345".to_string());
        assert!(err.to_string().contains("Block not found"));
        assert!(err.to_string().contains("garbage-collected"));
        assert!(err.to_string().contains("archival node"));
    }

    #[test]
    fn test_rpc_error_unknown_chunk_display() {
        let err = RpcError::UnknownChunk("ChunkHash123".to_string());
        assert!(err.to_string().contains("Chunk not found"));
    }

    #[test]
    fn test_rpc_error_unknown_epoch_display() {
        let err = RpcError::UnknownEpoch("epoch 100".to_string());
        assert!(err.to_string().contains("Epoch not found"));
    }

    #[test]
    fn test_rpc_error_invalid_shard_id_display() {
        let err = RpcError::InvalidShardId("999".to_string());
        assert!(err.to_string().contains("Invalid shard ID"));
        assert!(err.to_string().contains("999"));
    }

    #[test]
    fn test_rpc_error_unknown_receipt_display() {
        let err = RpcError::UnknownReceipt("receipt123".to_string());
        assert!(err.to_string().contains("Receipt not found"));
    }

    #[test]
    fn test_rpc_error_insufficient_balance_display() {
        let err = RpcError::InsufficientBalance {
            required: "10 NEAR".to_string(),
            available: "5 NEAR".to_string(),
        };
        assert!(err.to_string().contains("Insufficient balance"));
        assert!(err.to_string().contains("10 NEAR"));
        assert!(err.to_string().contains("5 NEAR"));
    }

    #[test]
    fn test_rpc_error_gas_limit_exceeded_display() {
        let err = RpcError::GasLimitExceeded {
            gas_used: "100 Tgas".to_string(),
            gas_limit: "50 Tgas".to_string(),
        };
        assert!(err.to_string().contains("Gas limit exceeded"));
    }

    #[test]
    fn test_rpc_error_parse_error_display() {
        let err = RpcError::ParseError("invalid JSON".to_string());
        assert!(err.to_string().contains("Parse error"));
    }

    #[test]
    fn test_rpc_error_invalid_response_display() {
        let err = RpcError::InvalidResponse("missing result field".to_string());
        assert!(err.to_string().contains("Invalid response"));
    }

    #[test]
    fn test_rpc_error_invalid_account_display() {
        let err = RpcError::InvalidAccount("bad..account".to_string());
        assert!(err.to_string().contains("Invalid account ID"));
    }

    // ========================================================================
    // Main Error Type Tests
    // ========================================================================

    #[test]
    fn test_error_no_signer_display() {
        let err = Error::NoSigner;
        assert!(err.to_string().contains("No signer configured"));
        assert!(err.to_string().contains(".signer()"));
    }

    #[test]
    fn test_error_no_signer_account_display() {
        let err = Error::NoSignerAccount;
        assert!(err.to_string().contains("No signer account ID"));
    }

    #[test]
    fn test_error_config_display() {
        let err = Error::Config("invalid RPC URL".to_string());
        assert!(err.to_string().contains("Invalid configuration"));
        assert!(err.to_string().contains("invalid RPC URL"));
    }

    #[test]
    fn test_error_transaction_failed_display() {
        let err = Error::TransactionFailed("execution error".to_string());
        assert!(err.to_string().contains("Transaction failed"));
    }

    #[test]
    fn test_error_invalid_transaction_display() {
        let err = Error::InvalidTransaction("bad signature".to_string());
        assert!(err.to_string().contains("Invalid transaction"));
    }

    #[test]
    fn test_error_contract_panic_display() {
        let err = Error::ContractPanic("assertion failed".to_string());
        assert!(err.to_string().contains("Contract panic"));
    }

    #[test]
    fn test_error_borsh_display() {
        let err = Error::Borsh("deserialization failed".to_string());
        assert!(err.to_string().contains("Borsh error"));
    }

    #[test]
    fn test_error_from_parse_account_id_error() {
        let parse_err = ParseAccountIdError::Empty;
        let err: Error = parse_err.into();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_error_from_parse_amount_error() {
        let parse_err = ParseAmountError::Overflow;
        let err: Error = parse_err.into();
        assert!(err.to_string().contains("overflow"));
    }

    #[test]
    fn test_error_from_parse_gas_error() {
        let parse_err = ParseGasError::Overflow;
        let err: Error = parse_err.into();
        assert!(err.to_string().contains("overflow"));
    }

    #[test]
    fn test_error_from_parse_key_error() {
        let parse_err = ParseKeyError::InvalidFormat;
        let err: Error = parse_err.into();
        assert!(err.to_string().contains("ed25519"));
    }

    #[test]
    fn test_error_from_rpc_error() {
        let rpc_err = RpcError::Timeout(5);
        let err: Error = rpc_err.into();
        assert!(err.to_string().contains("Timeout"));
    }

    #[test]
    fn test_error_from_signer_error() {
        let signer_err = SignerError::InvalidSeedPhrase;
        let err: Error = signer_err.into();
        assert!(err.to_string().contains("seed phrase"));
    }

    #[test]
    fn test_error_from_keystore_error() {
        let account_id: AccountId = "test.near".parse().unwrap();
        let keystore_err = KeyStoreError::KeyNotFound(account_id);
        let err: Error = keystore_err.into();
        assert!(err.to_string().contains("Key not found"));
    }
}
