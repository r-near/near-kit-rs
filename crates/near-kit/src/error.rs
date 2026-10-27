//! Error types for near-kit.
//!
//! This module provides comprehensive error types for all near-kit operations.
//!
//! # Error Hierarchy
//!
//! - [`Error`](enum@Error) — Main error type, returned by most operations
//!   - [`RpcError`] — RPC/network errors (connectivity, account not found, etc.)
//!   - [`InvalidTxError`][crate::protocol::InvalidTxError] — Transaction was rejected
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
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::protocol::Action;
//!
//! # async fn example(alice: SigningNear) -> Result<(), Error> {
//! let transfer = alice
//!     .transaction(account!("bob.testnet"))
//!     .add_action(Action::transfer(NearToken::from_near(1)));
//!
//! match transfer.await {
//!     Ok(outcome) if outcome.is_success() => {
//!         println!("Success! Hash: {}", outcome.transaction_hash());
//!     }
//!     Ok(outcome) => {
//!         // Transaction executed but an action failed — inspect the outcome
//!         println!("Action failed: {:?}, gas used: {}", outcome.failure_message(), outcome.total_gas_used());
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
//! use near_kit::rpc::RpcError;
//!
//! fn should_retry(err: &RpcError) -> bool {
//!     err.is_retryable()
//! }
//! ```

use thiserror::Error;

use crate::types::{
    AccountId, CryptoHash, FinalExecutionOutcome, GlobalContractIdentifierView, InvalidTxError,
    PublicKey, TxExecutionStatus,
};

// The client-free error types live in near-kit-primitives (each is public in
// near-kit through the facade modules). They are imported here so the
// `From` impls below, and `crate::error::*` paths elsewhere, keep working.
pub(crate) use near_kit_primitives::protocol::{DecodeTransactionError, ParseAccountIdError};
pub(crate) use near_kit_primitives::rpc::OutcomeError;
pub(crate) use near_kit_primitives::signer::{ParseKeyError, SignerError};
pub(crate) use near_kit_primitives::transaction::{CodecError, ContractError};

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
// `non_exhaustive` keeps the `rpc` feature additive: the `Http` variant below
// only exists with the feature on, so an exhaustive downstream match written
// against the offline build would otherwise break when feature unification
// turns `rpc` on.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcError {
    // ─── Network/Transport ───
    // reqwest only compiles with the `rpc` feature and never on WASI (the
    // wasi:http transport reports failures as `Network` instead), so this
    // variant (and its `is_retryable` arm below) exists only there.
    #[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
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
    #[error("Account not found: {account_id}")]
    AccountNotFound {
        account_id: AccountId,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// The requested account ID is malformed (`INVALID_ACCOUNT`).
    ///
    /// `account_id` is the raw string the node rejected; it is not an
    /// [`AccountId`] because it failed validation.
    #[error("Invalid account ID: {account_id}")]
    InvalidAccount {
        account_id: String,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// The access key does not exist on the account (`UNKNOWN_ACCESS_KEY`).
    ///
    /// `EXPERIMENTAL_view_access_key` omits the account from its payload; the
    /// typed helpers patch in the requested account, so it is only `unknown`
    /// when parsing a raw response directly.
    #[error("Access key not found: {account_id} / {public_key}")]
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// The gas key does not exist on the account (`UNKNOWN_GAS_KEY`, from
    /// `view_gas_key_nonces`).
    ///
    /// The wire payload only carries the public key; the typed helpers patch
    /// in the requested account, so it is only `unknown` when parsing a raw
    /// response directly.
    #[error("Gas key not found: {account_id} / {public_key}")]
    GasKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    // ─── Contract Errors ───
    #[error("Contract not deployed on account: {account_id}")]
    ContractNotDeployed {
        account_id: AccountId,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// The node refused an unpaginated `view_state` because the contract's
    /// state exceeds its limit (`TOO_LARGE_CONTRACT_STATE`). Use the paginated
    /// form instead.
    #[error("Contract state too large for account: {account_id}")]
    ContractStateTooLarge {
        account_id: AccountId,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// No global contract is published under `identifier`
    /// (`NO_GLOBAL_CONTRACT_CODE`).
    #[error("Global contract not found: {identifier}")]
    GlobalContractNotFound {
        identifier: GlobalContractIdentifierView,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// A view function failed for a reason that does not have a more specific
    /// [`RpcError`] variant.
    ///
    /// Missing contract code, missing methods, and contract panics are exposed
    /// as [`ContractNotDeployed`](Self::ContractNotDeployed),
    /// [`MethodNotFound`](Self::MethodNotFound), and
    /// [`ContractPanic`](Self::ContractPanic), respectively. Those are
    /// recognized from the node's structured `vm_error` field and, when a
    /// provider only returns the legacy string form (for example
    /// `Function call returned an error: MethodResolveError(MethodNotFound)`),
    /// from that string as well. This variant is the catch-all left after both
    /// classifications, so matching on `message` for those cases is not needed.
    #[error("contract execution failed on {contract_id}: {message}")]
    ContractExecution {
        contract_id: AccountId,
        method_name: Option<String>,
        message: String,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// The requested method is not exported by the deployed contract.
    #[error("contract method not found: `{contract_id}::{method_name}`")]
    MethodNotFound {
        contract_id: AccountId,
        method_name: String,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },

    /// A view method explicitly panicked during contract execution.
    ///
    /// Transaction function-call panics are reported in the transaction
    /// outcome instead; see [`crate::protocol::FunctionCallError`].
    #[error("contract panic: {message}")]
    ContractPanic {
        message: String,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
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
    /// error could not be deserialized into [`InvalidTxError`][crate::protocol::InvalidTxError].
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

    /// The node timed out waiting for a transaction to reach the requested
    /// wait level (`TIMEOUT_ERROR` from `send_tx` / `tx_status`).
    ///
    /// This does **not** mean the transaction failed: it may still land.
    /// Inspect [`cause`](TimeoutCause) to see how far it got, and use
    /// `transaction_hash` to poll `tx_status` instead of signing a new
    /// transaction.
    #[error(
        "Request timeout: {message}{}",
        .cause.as_ref().map(|c| format!(" ({c})")).unwrap_or_default()
    )]
    RequestTimeout {
        /// The JSON-RPC error message.
        message: String,
        /// Hash of the transaction waited on. Filled in by `send_tx`/`tx_status`;
        /// nearcore doesn't send it.
        transaction_hash: Option<CryptoHash>,
        /// Why the node timed out, as reported by nearcore 2.14+.
        ///
        /// `None` when the node predates 2.14 (which sent a bare
        /// `TIMEOUT_ERROR`) or omitted the cause.
        cause: Option<TimeoutCause>,
    },

    // ─── Access Key Listing ───
    /// An unpaginated `view_access_key_list` request hit the node's cap.
    ///
    /// nearcore only returns an account's keys in one shot when there are at
    /// most `limit` of them (100 by default); anything larger has to be listed
    /// with `limit`/`after_key`. `RpcClient::view_access_key_list` (and
    /// `Near::access_keys`) always paginate, so this only surfaces from
    /// `RpcClient::view_access_key_list_page` called without a `limit`.
    #[error(
        "account {account_id} has more than {limit} access keys; use a paginated view_access_key_list request"
    )]
    TooManyAccessKeys {
        account_id: AccountId,
        limit: u32,
        block_height: Option<u64>,
        block_hash: Option<CryptoHash>,
    },
}

/// Why a node returned `TIMEOUT_ERROR` for a transaction (nearcore 2.14+).
///
/// Carried by [`RpcError::RequestTimeout`]. Mirrors nearcore's
/// `TimeoutErrorCause`, which arrives on the wire as
/// `error.cause.info = {"cause": "<TAG>", ...}`.
///
/// # Example
///
/// ```rust
/// use near_kit::rpc::{RpcError, TimeoutCause};
///
/// fn describe(err: &RpcError) -> &'static str {
///     match err {
///         RpcError::RequestTimeout { cause: Some(TimeoutCause::Pending { .. }), .. } => {
///             "accepted, still waiting for finality: poll tx_status by hash"
///         }
///         RpcError::RequestTimeout { cause: Some(TimeoutCause::NotObserved), .. } => {
///             "not on chain yet: re-sending the same signed transaction is safe"
///         }
///         RpcError::RequestTimeout { .. } => "timed out",
///         _ => "other error",
///     }
/// }
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TimeoutCause {
    /// The node never observed the transaction on chain.
    NotObserved,
    /// The transaction was observed but had not reached the requested wait
    /// level.
    ///
    /// Do not re-sign: the transaction has already been accepted.
    Pending {
        /// The wait level the transaction had reached.
        final_execution_status: TxExecutionStatus,
        /// The execution outcome known so far, if the node had one.
        outcome: Option<Box<FinalExecutionOutcome>>,
    },
    /// The node does not track the transaction's shard and got no answer
    /// from a node that does before the timeout.
    DoesNotTrackShard {
        /// The shard the transaction belongs to.
        shard_id: u64,
    },
    /// The node could not produce a usable status before the timeout (for
    /// example a repeated internal error).
    Error {
        /// Free-form diagnostic text from the node.
        debug_info: String,
    },
    /// A cause near-kit does not recognise, or one whose payload failed to
    /// decode. Holds the raw `error.cause.info` JSON.
    Unknown(serde_json::Value),
}

impl std::fmt::Display for TimeoutCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeoutCause::NotObserved => write!(f, "transaction not observed"),
            TimeoutCause::Pending {
                final_execution_status,
                ..
            } => write!(
                f,
                "transaction pending, reached {}",
                final_execution_status.as_str()
            ),
            TimeoutCause::DoesNotTrackShard { shard_id } => {
                write!(f, "node does not track shard {shard_id}")
            }
            TimeoutCause::Error { debug_info } => write!(f, "{debug_info}"),
            TimeoutCause::Unknown(raw) => write!(f, "unknown cause: {raw}"),
        }
    }
}

impl RpcError {
    /// Check if this error is retryable.
    ///
    /// "Retryable" means re-sending the *same* request may succeed: transient
    /// transport failures, timeouts, and server-side conditions like an
    /// unsynced node or a 5xx. This is what `RpcClient::call` consults
    /// between attempts.
    ///
    /// For [`RpcError::InvalidTx`] the signed payload is fixed at this layer,
    /// so only rejections that can clear while the *same* bytes are re-sent
    /// count: `ShardCongested` and `ShardStuck`. `InvalidNonce` is transient
    /// too ([`InvalidTxError::is_retryable`] says so) but re-sending the
    /// rejected transaction byte-for-byte can never succeed — it needs a
    /// fresh nonce and a new signature. That refresh-and-re-sign loop lives
    /// in the transaction layer (`SigningNear` sends), where retrying can actually
    /// change the outcome, so it is `false` here.
    ///
    /// A connection the server reset, aborted, or closed mid-exchange counts
    /// as transient; overloaded nodes commonly shed load this way. The node
    /// may already have handled such a request, so `RpcClient::call` never
    /// retries `sandbox_fast_forward`, the one method that is not idempotent.
    ///
    /// [`InvalidTxError::is_retryable`]: crate::protocol::InvalidTxError::is_retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            #[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
            RpcError::Http(e) => {
                // `is_connect()` doesn't exist on reqwest's JS fetch backend
                // (`wasm32-unknown-unknown` — no hyper connector); `is_request()`
                // covers transport-level failures there (e.g. offline, DNS,
                // connection reset) so retries still work.
                e.is_timeout() || {
                    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                    {
                        e.is_connect() || is_connection_dropped(e)
                    }
                    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                    {
                        e.is_request()
                    }
                }
            }
            RpcError::Timeout(_) => true,
            RpcError::Network { retryable, .. } => *retryable,
            RpcError::ShardUnavailable(_) => true,
            RpcError::NodeNotSynced(_) => true,
            RpcError::InternalError(_) => true,
            RpcError::RequestTimeout { .. } => true,
            // The payload is fixed here, so `InvalidNonce` can't be fixed by
            // re-sending it (the transaction layer re-signs instead); shard
            // congestion/stalls can clear on their own. See the doc comment.
            RpcError::InvalidTx(InvalidTxError::InvalidNonce { .. }) => false,
            RpcError::InvalidTx(e) => e.is_retryable(),
            RpcError::Rpc { code, .. } => {
                // Retry on server errors
                *code == -32000 || *code == -32603
            }
            _ => false,
        }
    }

    // Only the `rpc` client builds these three.
    #[cfg_attr(not(feature = "rpc"), allow(dead_code))]
    /// Create a network error.
    pub(crate) fn network(
        message: impl Into<String>,
        status_code: Option<u16>,
        retryable: bool,
    ) -> Self {
        RpcError::Network {
            message: message.into(),
            status_code,
            retryable,
        }
    }

    #[cfg_attr(not(feature = "rpc"), allow(dead_code))]
    /// Create an invalid transaction error, attempting to parse structured data first.
    pub(crate) fn invalid_transaction(
        message: impl Into<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        // Try to deserialize the structured error from the data field
        if let Some(ref data) = details
            && let Some(invalid_tx) = Self::try_parse_invalid_tx(data)
        {
            return RpcError::InvalidTx(invalid_tx);
        }

        RpcError::InvalidTransaction {
            message: message.into(),
            details,
        }
    }

    #[cfg_attr(not(feature = "rpc"), allow(dead_code))]
    /// Try to extract an `InvalidTxError` from the RPC error data JSON.
    ///
    /// The data field typically looks like:
    /// `{"TxExecutionError": {"InvalidTxError": {"InvalidNonce": {...}}}}`
    fn try_parse_invalid_tx(data: &serde_json::Value) -> Option<crate::types::InvalidTxError> {
        // Nested form: data.TxExecutionError.InvalidTxError
        if let Some(tx_err) = data.get("TxExecutionError")
            && let Some(invalid_tx_value) = tx_err.get("InvalidTxError")
            && let Ok(parsed) = serde_json::from_value(invalid_tx_value.clone())
        {
            return Some(parsed);
        }

        // Fallback: InvalidTxError at the top level (some RPC versions)
        if let Some(invalid_tx_value) = data.get("InvalidTxError")
            && let Ok(parsed) = serde_json::from_value(invalid_tx_value.clone())
        {
            return Some(parsed);
        }

        None
    }
}

/// Whether a reqwest error was caused by the peer dropping an established
/// connection: a reset, an abort, a broken pipe, or the connection closing
/// before the response was complete.
///
/// reqwest does not classify these (`is_connect()` only covers failures to
/// establish a connection), so walk the source chain to the underlying
/// [`std::io::Error`] or hyper error. hyper reports a pooled keep-alive
/// connection that the server closed mid-request as an "incomplete message"
/// with no I/O error underneath; nodes under parallel load do this routinely.
#[cfg(all(
    feature = "rpc",
    not(all(target_arch = "wasm32", target_os = "wasi")),
    not(all(target_arch = "wasm32", target_os = "unknown"))
))]
fn is_connection_dropped(error: &reqwest::Error) -> bool {
    use std::io::ErrorKind;

    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(err) = source {
        if let Some(io) = err.downcast_ref::<std::io::Error>()
            && matches!(
                io.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
                    | ErrorKind::UnexpectedEof
            )
        {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if err
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_incomplete_message)
        {
            return true;
        }
        source = err.source();
    }
    false
}

// ============================================================================
// Main Error Type
// ============================================================================

/// Main error type for near-kit operations.
impl RpcError {
    /// Returns true if this error indicates the account was not found.
    pub fn is_account_not_found(&self) -> bool {
        matches!(self, RpcError::AccountNotFound { .. })
    }

    /// Returns true if this error indicates a contract is not deployed.
    pub fn is_contract_not_deployed(&self) -> bool {
        matches!(self, RpcError::ContractNotDeployed { .. })
    }

    /// Returns true if this error indicates the requested contract method was
    /// not found.
    pub fn is_method_not_found(&self) -> bool {
        matches!(self, RpcError::MethodNotFound { .. })
    }

    /// Returns true if this error indicates a view method panicked.
    pub fn is_contract_panic(&self) -> bool {
        matches!(self, RpcError::ContractPanic { .. })
    }

    /// Returns true if this error indicates a global contract was not found.
    pub fn is_global_contract_not_found(&self) -> bool {
        matches!(self, RpcError::GlobalContractNotFound { .. })
    }

    /// The variant name, for structured `tracing` fields.
    ///
    /// Used instead of the `Display` text in RPC-layer events: some variants
    /// are only fully populated after the typed helpers patch in caller-known
    /// context (e.g. [`MethodNotFound`](Self::MethodNotFound) reads
    /// `unknown::unknown` straight out of the parser), so their message would
    /// be misleading in a log line.
    #[cfg(all(feature = "rpc", feature = "tracing"))]
    pub(crate) fn variant_name(&self) -> &'static str {
        match self {
            #[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
            RpcError::Http(_) => "Http",
            RpcError::Network { .. } => "Network",
            RpcError::Timeout(_) => "Timeout",
            RpcError::Json(_) => "Json",
            RpcError::InvalidResponse(_) => "InvalidResponse",
            RpcError::Rpc { .. } => "Rpc",
            RpcError::AccountNotFound { .. } => "AccountNotFound",
            RpcError::InvalidAccount { .. } => "InvalidAccount",
            RpcError::AccessKeyNotFound { .. } => "AccessKeyNotFound",
            RpcError::GasKeyNotFound { .. } => "GasKeyNotFound",
            RpcError::ContractNotDeployed { .. } => "ContractNotDeployed",
            RpcError::ContractStateTooLarge { .. } => "ContractStateTooLarge",
            RpcError::GlobalContractNotFound { .. } => "GlobalContractNotFound",
            RpcError::ContractExecution { .. } => "ContractExecution",
            RpcError::MethodNotFound { .. } => "MethodNotFound",
            RpcError::ContractPanic { .. } => "ContractPanic",
            RpcError::UnknownBlock(_) => "UnknownBlock",
            RpcError::UnknownChunk(_) => "UnknownChunk",
            RpcError::UnknownEpoch(_) => "UnknownEpoch",
            RpcError::InvalidShardId(_) => "InvalidShardId",
            RpcError::UnknownReceipt(_) => "UnknownReceipt",
            RpcError::InvalidTx(_) => "InvalidTx",
            RpcError::InvalidTransaction { .. } => "InvalidTransaction",
            RpcError::ShardUnavailable(_) => "ShardUnavailable",
            RpcError::NodeNotSynced(_) => "NodeNotSynced",
            RpcError::InternalError(_) => "InternalError",
            RpcError::ParseError(_) => "ParseError",
            RpcError::RequestTimeout { .. } => "RequestTimeout",
            RpcError::TooManyAccessKeys { .. } => "TooManyAccessKeys",
        }
    }

    /// Returns the block height attached to a view RPC error, if available.
    pub fn block_height(&self) -> Option<u64> {
        match self {
            RpcError::AccountNotFound { block_height, .. }
            | RpcError::InvalidAccount { block_height, .. }
            | RpcError::AccessKeyNotFound { block_height, .. }
            | RpcError::GasKeyNotFound { block_height, .. }
            | RpcError::ContractNotDeployed { block_height, .. }
            | RpcError::ContractStateTooLarge { block_height, .. }
            | RpcError::GlobalContractNotFound { block_height, .. }
            | RpcError::ContractExecution { block_height, .. }
            | RpcError::MethodNotFound { block_height, .. }
            | RpcError::ContractPanic { block_height, .. }
            | RpcError::TooManyAccessKeys { block_height, .. } => *block_height,
            _ => None,
        }
    }

    /// Returns the block hash attached to a view RPC error, if available.
    pub fn block_hash(&self) -> Option<CryptoHash> {
        match self {
            RpcError::AccountNotFound { block_hash, .. }
            | RpcError::InvalidAccount { block_hash, .. }
            | RpcError::AccessKeyNotFound { block_hash, .. }
            | RpcError::GasKeyNotFound { block_hash, .. }
            | RpcError::ContractNotDeployed { block_hash, .. }
            | RpcError::ContractStateTooLarge { block_hash, .. }
            | RpcError::GlobalContractNotFound { block_hash, .. }
            | RpcError::ContractExecution { block_hash, .. }
            | RpcError::MethodNotFound { block_hash, .. }
            | RpcError::ContractPanic { block_hash, .. }
            | RpcError::TooManyAccessKeys { block_hash, .. } => *block_hash,
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    // ─── Configuration ───
    #[error("Invalid configuration: {0}")]
    Config(String),

    // ─── Parsing ───
    #[error(transparent)]
    ParseAccountId(#[from] ParseAccountIdError),

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
    Signing(#[source] SignerError),

    // ─── KeyStore ───
    #[error(transparent)]
    KeyStore(#[from] KeyStoreError),

    // ─── Serialization ───
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Borsh error: {0}")]
    Borsh(String),
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

impl From<SignerError> for Error {
    fn from(err: SignerError) -> Self {
        match err {
            SignerError::InvalidKey(e) => Error::ParseKey(e),
            SignerError::NoKeys | SignerError::MixedAccounts { .. } => {
                Error::Config(err.to_string())
            }
            other => Error::Signing(other),
        }
    }
}

impl From<CodecError> for Error {
    fn from(err: CodecError) -> Self {
        match err {
            CodecError::Json(e) => Error::Json(e),
            CodecError::Borsh(e) => Error::Borsh(e.to_string()),
            other => Error::InvalidTransaction(other.to_string()),
        }
    }
}

/// Lets `?` turn a contract client's error into an [`enum@Error`]: encoding and
/// decoding failures map like [`CodecError`], backend errors through their own
/// `Into<Error>` (`RpcError` becomes [`Error::Rpc`]).
impl<E: Into<Error> + std::fmt::Display> From<ContractError<E>> for Error {
    fn from(err: ContractError<E>) -> Self {
        match err {
            ContractError::Codec(e) => e.into(),
            ContractError::Backend(e) => e.into(),
            other => Error::InvalidTransaction(other.to_string()),
        }
    }
}

impl From<DecodeTransactionError> for Error {
    fn from(err: DecodeTransactionError) -> Self {
        Error::InvalidTransaction(err.to_string())
    }
}

impl From<OutcomeError> for Error {
    fn from(err: OutcomeError) -> Self {
        match err {
            OutcomeError::InvalidTx(e) => Error::InvalidTx(e),
            OutcomeError::Json(e) => Error::Json(e),
            other => Error::InvalidTransaction(other.to_string()),
        }
    }
}

impl Error {
    /// The underlying [`RpcError`], if this is an [`Error::Rpc`].
    ///
    /// Queries such as `Near::balance`, signed sends, and views converted with
    /// `?` or `Error::from` give an [`Error`](crate::Error), which boxes the
    /// RPC error. Use this to inspect it without matching through the box.
    ///
    /// Structured transaction validation errors are promoted to
    /// [`Error::InvalidTx`] and are not returned here.
    ///
    /// # Example
    ///
    #[cfg_attr(feature = "rpc", doc = "```rust,no_run")]
    #[cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
    /// # use near_kit::Near;
    /// # async fn example(near: &Near) -> Result<(), near_kit::Error> {
    /// # use near_kit::account;
    /// match near.view::<u64>(account!("counter.testnet"), "get_count").await.map_err(near_kit::Error::from) {
    ///     Ok(count) => println!("{count}"),
    ///     Err(e) if e.as_rpc().is_some_and(|e| e.is_contract_not_deployed()) => {
    ///         println!("no contract");
    ///     }
    ///     Err(e) => return Err(e),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn as_rpc(&self) -> Option<&RpcError> {
        match self {
            Error::Rpc(error) => Some(error),
            _ => None,
        }
    }

    /// Unwrap the underlying [`RpcError`] by value, or give the error back.
    ///
    /// Use this to match RPC variants and move their fields out:
    ///
    #[cfg_attr(feature = "rpc", doc = "```rust,no_run")]
    #[cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
    /// # use near_kit::{Error, Near, account};
    /// # use near_kit::rpc::RpcError;
    /// # async fn example(near: &Near) -> Result<(), Error> {
    /// let result = near.view::<String>(account!("resolver.testnet"), "resolve").await;
    /// match result.map_err(Error::from).map_err(Error::into_rpc) {
    ///     Ok(value) => println!("{value}"),
    ///     Err(Ok(RpcError::ContractPanic { message, .. })) => println!("panicked: {message}"),
    ///     Err(Ok(other)) => return Err(other.into()),
    ///     Err(Err(other)) => return Err(other),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_rpc(self) -> Result<RpcError, Self> {
        match self {
            Error::Rpc(error) => Ok(*error),
            other => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FinalExecutionStatus;

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
            RpcError::AccountNotFound {
                account_id: account_id.clone(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "Account not found: alice.near"
        );
        assert_eq!(
            RpcError::InvalidAccount {
                account_id: "bad-account".to_string(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "Invalid account ID: bad-account"
        );
        assert_eq!(
            RpcError::ContractNotDeployed {
                account_id: account_id.clone(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "Contract not deployed on account: alice.near"
        );
        assert_eq!(
            RpcError::ContractStateTooLarge {
                account_id: account_id.clone(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
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
                cause: None,
            }
            .is_retryable()
        );
        // Shard congestion can clear while the same signed bytes are re-sent.
        assert!(
            RpcError::InvalidTx(InvalidTxError::ShardCongested {
                congestion_level: 1.0,
                shard_id: 0,
            })
            .is_retryable()
        );
        assert!(
            RpcError::InvalidTx(InvalidTxError::ShardStuck {
                missed_chunks: 3,
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
        assert!(
            !RpcError::AccountNotFound {
                account_id: account_id.clone(),
                block_height: None,
                block_hash: None,
            }
            .is_retryable()
        );
        assert!(
            !RpcError::ContractNotDeployed {
                account_id: account_id.clone(),
                block_height: None,
                block_hash: None,
            }
            .is_retryable()
        );
        assert!(
            !RpcError::InvalidAccount {
                account_id: "bad".to_string(),
                block_height: None,
                block_hash: None,
            }
            .is_retryable()
        );
        assert!(!RpcError::UnknownBlock("12345".to_string()).is_retryable());
        assert!(!RpcError::ParseError("bad json".to_string()).is_retryable());
        // InvalidNonce is transient at the transaction layer (re-sign with a
        // fresh nonce) but terminal here: the signed payload can't change
        // between attempts, so re-sending it can't succeed.
        assert!(
            !RpcError::InvalidTx(InvalidTxError::InvalidNonce {
                tx_nonce: 5,
                ak_nonce: 10
            })
            .is_retryable()
        );
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
    fn test_rpc_error_is_account_not_found() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert!(
            RpcError::AccountNotFound {
                account_id,
                block_height: None,
                block_hash: None,
            }
            .is_account_not_found()
        );
        assert!(!RpcError::Timeout(3).is_account_not_found());
    }

    #[test]
    fn test_rpc_error_is_contract_not_deployed() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        assert!(
            RpcError::ContractNotDeployed {
                account_id,
                block_height: None,
                block_hash: None,
            }
            .is_contract_not_deployed()
        );
        assert!(!RpcError::Timeout(3).is_contract_not_deployed());
    }

    #[test]
    fn test_rpc_error_view_call_classification_helpers() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let block_hash = CryptoHash::hash(b"block");
        let method_not_found = RpcError::MethodNotFound {
            contract_id: account_id,
            method_name: "missing".to_string(),
            block_height: Some(42),
            block_hash: Some(block_hash),
        };
        assert!(method_not_found.is_method_not_found());
        assert!(!method_not_found.is_contract_panic());
        assert_eq!(method_not_found.block_height(), Some(42));
        assert_eq!(method_not_found.block_hash(), Some(block_hash));

        let panic = RpcError::ContractPanic {
            message: "boom".to_string(),
            block_height: None,
            block_hash: None,
        };
        assert!(panic.is_contract_panic());
        assert!(!panic.is_method_not_found());
        assert_eq!(panic.block_height(), None);
        assert_eq!(panic.block_hash(), None);
        assert_eq!(RpcError::Timeout(3).block_height(), None);
        assert_eq!(RpcError::Timeout(3).block_hash(), None);
    }

    #[test]
    fn test_rpc_error_block_context_accessors_cover_handler_errors() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let block_hash = CryptoHash::hash(b"block");
        let errors = [
            RpcError::InvalidAccount {
                account_id: "bad@id".to_string(),
                block_height: Some(7),
                block_hash: Some(block_hash),
            },
            RpcError::AccessKeyNotFound {
                account_id: account_id.clone(),
                public_key,
                block_height: Some(7),
                block_hash: Some(block_hash),
            },
            RpcError::ContractStateTooLarge {
                account_id: account_id.clone(),
                block_height: Some(7),
                block_hash: Some(block_hash),
            },
            RpcError::GlobalContractNotFound {
                identifier: GlobalContractIdentifierView::AccountId(account_id),
                block_height: Some(7),
                block_hash: Some(block_hash),
            },
        ];
        for err in errors {
            assert_eq!(err.block_height(), Some(7), "{err:?}");
            assert_eq!(err.block_hash(), Some(block_hash), "{err:?}");
            assert!(!err.is_retryable(), "{err:?}");
        }
    }

    #[test]
    fn test_rpc_error_method_not_found_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::MethodNotFound {
            contract_id: account_id,
            method_name: "missing".to_string(),
            block_height: None,
            block_hash: None,
        };
        assert_eq!(
            err.to_string(),
            "contract method not found: `contract.near::missing`"
        );
    }

    #[test]
    fn test_rpc_error_contract_execution_display() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let err = RpcError::ContractExecution {
            contract_id: account_id,
            method_name: Some("my_method".to_string()),
            message: "execution failed".to_string(),
            block_height: None,
            block_hash: None,
        };
        assert_eq!(
            err.to_string(),
            "contract execution failed on contract.near: execution failed"
        );
    }

    #[test]
    fn test_rpc_error_contract_panic_display() {
        let panic = RpcError::ContractPanic {
            message: "assertion failed".to_string(),
            block_height: None,
            block_hash: None,
        };
        assert_eq!(panic.to_string(), "contract panic: assertion failed");
    }

    #[test]
    fn test_view_call_error_display_preserves_message_casing() {
        let contract_id: AccountId = "contract.near".parse().unwrap();

        // Messages that come from the node are embedded exactly as they
        // arrived — nothing re-cases the first letter.
        assert_eq!(
            RpcError::ContractPanic {
                message: "ERR_NOT_ENOUGH_DEPOSIT".to_string(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "contract panic: ERR_NOT_ENOUGH_DEPOSIT"
        );
        assert_eq!(
            RpcError::ContractExecution {
                contract_id: contract_id.clone(),
                method_name: Some("ft_balance_of".to_string()),
                message: "Memory access violation".to_string(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "contract execution failed on contract.near: Memory access violation"
        );
        assert_eq!(
            RpcError::MethodNotFound {
                contract_id,
                method_name: "Nep413GetMessage".to_string(),
                block_height: None,
                block_hash: None,
            }
            .to_string(),
            "contract method not found: `contract.near::Nep413GetMessage`"
        );
    }

    #[test]
    fn test_rpc_error_invalid_tx_display() {
        use crate::types::InvalidTxError;
        let err = RpcError::InvalidTx(InvalidTxError::InvalidNonce {
            tx_nonce: 5,
            ak_nonce: 10,
        });
        assert!(err.to_string().contains("transaction nonce"));
        assert!(err.to_string().contains("access key nonce"));
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
            block_height: None,
            block_hash: None,
        };
        assert!(err.to_string().contains("alice.near"));
        assert!(err.to_string().contains(&public_key.to_string()));
    }

    #[test]
    fn test_rpc_error_gas_key_not_found_display() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let err = RpcError::GasKeyNotFound {
            account_id,
            public_key: public_key.clone(),
            block_height: Some(7),
            block_hash: Some(CryptoHash::ZERO),
        };
        assert_eq!(
            err.to_string(),
            format!("Gas key not found: alice.near / {public_key}")
        );
        assert_eq!(err.block_height(), Some(7));
        assert_eq!(err.block_hash(), Some(CryptoHash::ZERO));
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_rpc_error_request_timeout_display() {
        let err = RpcError::RequestTimeout {
            message: "request timed out".to_string(),
            transaction_hash: Some(CryptoHash::ZERO),
            cause: None,
        };
        assert_eq!(err.to_string(), "Request timeout: request timed out");

        let err = RpcError::RequestTimeout {
            message: "Server error".to_string(),
            transaction_hash: None,
            cause: Some(TimeoutCause::Pending {
                final_execution_status: crate::types::TxExecutionStatus::Included,
                outcome: None,
            }),
        };
        assert_eq!(
            err.to_string(),
            "Request timeout: Server error (transaction pending, reached INCLUDED)"
        );
    }

    // ========================================================================
    // Error (main type) tests
    // ========================================================================

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

    // The layer errors convert into the variant (and message) the same
    // failure produced before they existed, so `?` into `Error` is unchanged.

    #[test]
    fn test_error_from_signer_construction_errors() {
        let err: Error = "nope"
            .parse::<crate::signer::SecretKey>()
            .unwrap_err()
            .into();
        assert!(matches!(err, Error::ParseKey(_)));

        let err: Error = SignerError::NoKeys.into();
        assert_eq!(
            err.to_string(),
            "Invalid configuration: RotatingSigner requires at least one key"
        );

        let err: Error = SignerError::MixedAccounts {
            expected: "a.near".parse().unwrap(),
            found: "b.near".parse().unwrap(),
        }
        .into();
        assert_eq!(
            err.to_string(),
            "Invalid configuration: All signers must share the same account ID, got a.near and b.near"
        );
    }

    #[test]
    fn test_error_from_decode_transaction_error() {
        let err: Error = crate::protocol::SignedTransaction::from_base64("!!")
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::InvalidTransaction(m) if m.starts_with("Invalid base64: ")),
            "{err:?}"
        );

        let err: Error = crate::protocol::SignedTransaction::from_bytes(&[1, 2, 3])
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::InvalidTransaction(m) if m.starts_with("Failed to deserialize signed transaction: ")),
            "{err:?}"
        );
    }

    #[test]
    fn test_error_from_outcome_error() {
        let err: Error = OutcomeError::InvalidTx(Box::new(InvalidTxError::Expired)).into();
        assert!(matches!(err, Error::InvalidTx(_)));

        let json = serde_json::from_str::<u8>("x").unwrap_err();
        let err: Error = OutcomeError::Json(json).into();
        assert!(matches!(err, Error::Json(_)));

        let err: Error = OutcomeError::NotSuccess(Box::new(FinalExecutionStatus::Started)).into();
        assert!(
            matches!(&err, Error::InvalidTransaction(m) if m == "Transaction status is Started, expected SuccessValue"),
            "{err:?}"
        );

        let err: Error = OutcomeError::MissingOutcome {
            transaction_hash: CryptoHash::default(),
            wait_level: "Final",
        }
        .into();
        assert!(
            matches!(&err, Error::InvalidTransaction(m) if m.starts_with("RPC returned no execution outcome for transaction ") && m.ends_with(" at wait level Final")),
            "{err:?}"
        );
    }

    #[test]
    fn test_error_from_contract_error() {
        let err: Error =
            ContractError::<RpcError>::Codec(CodecError::Borsh(std::io::Error::other("bad")))
                .into();
        assert!(matches!(&err, Error::Borsh(m) if m == "bad"), "{err:?}");

        let err: Error = ContractError::Backend(RpcError::ContractPanic {
            message: "boom".to_string(),
            block_height: None,
            block_hash: None,
        })
        .into();
        assert!(err.as_rpc().is_some_and(RpcError::is_contract_panic));

        let err: Error = ContractError::Backend(Error::Config("x".to_string())).into();
        assert!(matches!(err, Error::Config(m) if m == "x"));
    }

    #[test]
    fn test_error_from_codec_error() {
        use crate::transaction::FunctionCall;

        struct FailingJson;
        impl serde::Serialize for FailingJson {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("deliberate"))
            }
        }

        let err: Error = FunctionCall::new("m")
            .args(FailingJson)
            .into_action()
            .unwrap_err()
            .into();
        assert!(matches!(err, Error::Json(_)));

        let err: Error = CodecError::Borsh(std::io::Error::other("bad")).into();
        assert!(matches!(&err, Error::Borsh(m) if m == "bad"), "{err:?}");
    }

    #[test]
    fn test_error_rpc_accessors() {
        let err: Error = RpcError::ContractPanic {
            message: "boom".to_string(),
            block_height: Some(7),
            block_hash: None,
        }
        .into();
        assert!(err.as_rpc().is_some_and(RpcError::is_contract_panic));
        assert_eq!(err.as_rpc().and_then(RpcError::block_height), Some(7));
        match err.into_rpc() {
            Ok(RpcError::ContractPanic { message, .. }) => assert_eq!(message, "boom"),
            other => panic!("unexpected: {other:?}"),
        }

        let err = Error::Config("x".to_string());
        assert!(err.as_rpc().is_none());
        assert!(matches!(err.into_rpc(), Err(Error::Config(_))));

        // Structured tx validation errors are promoted out of `Error::Rpc`.
        let err: Error = RpcError::InvalidTx(InvalidTxError::Expired).into();
        assert!(err.as_rpc().is_none());
    }
}
