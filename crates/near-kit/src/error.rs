//! Error types for near-kit: [`Error`](enum@Error), [`RpcError`] and
//! [`KeyStoreError`]. The module is private; the types are public at the
//! crate root, `near_kit::rpc` and `near_kit::signer`.

use thiserror::Error;

use crate::types::{
    AccountId, CryptoHash, FinalExecutionOutcome, GlobalContractIdentifierView, InvalidTxError,
    PublicKey, TxExecutionStatus,
};

// The client-free error types live in near-kit-primitives (each is public in
// near-kit through the facade modules). They are imported here so the
// `From` impls below, and `crate::error::*` paths elsewhere, keep working.
pub(crate) use near_kit_primitives::ContractError;
pub(crate) use near_kit_primitives::protocol::{
    DecodeDelegateError, DecodeTransactionError, ParseAccountIdError,
};
pub(crate) use near_kit_primitives::rpc::OutcomeError;
pub(crate) use near_kit_primitives::signer::{ParseKeyError, SignerError};
pub(crate) use near_kit_primitives::transaction::CodecError;

/// Error loading a key from a key store.
///
/// Carried by [`Error::KeyStore`]. The system keyring loader
/// (`signer::from_keyring`, `keyring` feature) returns these; the
/// environment and credentials-file loaders report problems as
/// [`Error::Config`] or a parse error instead.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeyStoreError {
    /// The store has no key for this account and public key.
    #[error("Key not found for account: {0}")]
    KeyNotFound(AccountId),

    /// An entry exists but isn't a usable credential: it isn't JSON, has
    /// no `private_key` field, or holds a key other than the one requested.
    #[error("Invalid credential format: {0}")]
    InvalidFormat(String),

    /// The entry's `private_key` did not parse.
    #[error("Invalid key: {0}")]
    InvalidKey(#[from] ParseKeyError),

    /// The platform key store (macOS Keychain, Windows Credential Manager, the
    /// Linux Secret Service) could not be opened or read.
    #[error("Platform keyring error: {0}")]
    Platform(String),
}

/// An error from the JSON-RPC layer: the transport, the response, or an error
/// the node returned.
///
/// Node errors are matched on their `cause.name` (`UNKNOWN_ACCOUNT`,
/// `TIMEOUT_ERROR`, ...) and mapped to the variants below; a cause without a
/// variant of its own becomes [`Rpc`](Self::Rpc). Variants that answer a
/// view request carry the block the node evaluated it at, also available
/// through [`block_height`](Self::block_height) and
/// [`block_hash`](Self::block_hash).
///
/// [`is_retryable`](Self::is_retryable) says whether the same request can
/// succeed if sent again. Most methods wrap this type in
/// [`Error::Rpc`]; an `InvalidTx` from the node is promoted to
/// [`Error::InvalidTx`] instead.
// `non_exhaustive` keeps the `rpc` feature additive: the `Http` variant below
// only exists with the feature on, so an exhaustive downstream match written
// against the offline build would otherwise break when feature unification
// turns `rpc` on.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RpcError {
    // reqwest only compiles with the `rpc` feature and never on WASI (the
    // wasi:http transport reports failures as `Network` instead), so this
    // variant (and its `is_retryable` arm below) exists only there.
    /// The HTTP client failed: connecting, a timeout, or reading the response.
    ///
    /// Retryable when the request timed out, the connection could not be
    /// made, or the server dropped it. Not present on WASI, where transport
    /// failures are [`Network`](Self::Network).
    ///
    /// near-kit drops the request URL from the reqwest error, so an API key
    /// in the URL doesn't show up in its `Display` or `Debug`.
    #[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
    #[error("HTTP error: {0}")]
    Http(#[source] reqwest::Error),

    /// The transport failed, or the node answered with a non-2xx HTTP status
    /// whose body isn't a JSON-RPC error.
    ///
    /// A custom `RpcTransport` reports its own failures with this variant.
    #[error("Network error: {message}")]
    Network {
        /// What went wrong. For an HTTP status, `HTTP <status>: <body>`.
        message: String,
        /// The HTTP status, when the failure was a response.
        status_code: Option<u16>,
        /// What [`is_retryable`](Self::is_retryable) returns for this error.
        /// near-kit sets it for HTTP 408, 429 and 5xx, and for transient
        /// transport failures.
        retryable: bool,
    },

    /// The response body is not valid JSON, or doesn't have the expected shape.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// The response parsed but can't be used, for example a JSON-RPC
    /// response with neither `result` nor `error`.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// A JSON-RPC error that no other variant covers.
    ///
    /// Retryable for code `-32603`, and for `-32000` when `cause` is absent or
    /// names a transient condition.
    #[error("RPC error: {message} (code: {code})")]
    Rpc {
        /// The JSON-RPC error code, such as `-32000` (server error).
        code: i64,
        /// The error message.
        message: String,
        /// The error's `data` field, if present.
        data: Option<serde_json::Value>,
        /// The name of the node's error cause (`error.cause.name`, such as
        /// `NODE_IS_SYNCING`), when the response has one.
        cause: Option<String>,
    },

    /// The account does not exist at the queried block (`UNKNOWN_ACCOUNT`).
    #[error("Account not found: {account_id}")]
    AccountNotFound {
        /// The account requested.
        account_id: AccountId,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The requested account ID is malformed (`INVALID_ACCOUNT`).
    ///
    /// `account_id` is the raw string the node rejected; it is not an
    /// [`AccountId`] because it failed validation.
    #[error("Invalid account ID: {account_id}")]
    InvalidAccount {
        /// The account ID as the node received it.
        account_id: String,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The access key does not exist on the account (`UNKNOWN_ACCESS_KEY`).
    ///
    /// `EXPERIMENTAL_view_access_key` omits the account from its payload; the
    /// typed helpers patch in the requested account, so it is only `unknown`
    /// when parsing a raw response directly.
    #[error("Access key not found: {account_id} / {public_key}")]
    AccessKeyNotFound {
        /// The account queried.
        account_id: AccountId,
        /// The public key looked up.
        public_key: PublicKey,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
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
        /// The account queried.
        account_id: AccountId,
        /// The gas key's public key.
        public_key: PublicKey,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The account has no contract deployed (`NO_CONTRACT_CODE`).
    #[error("Contract not deployed on account: {account_id}")]
    ContractNotDeployed {
        /// The account queried.
        account_id: AccountId,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The node refused an unpaginated `view_state` because the contract's
    /// state exceeds its limit (`TOO_LARGE_CONTRACT_STATE`). Use the paginated
    /// form instead.
    #[error("Contract state too large for account: {account_id}")]
    ContractStateTooLarge {
        /// The contract account.
        account_id: AccountId,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// No global contract is published under `identifier`
    /// (`NO_GLOBAL_CONTRACT_CODE`).
    #[error("Global contract not found: {identifier}")]
    GlobalContractNotFound {
        /// The global contract looked up, by code hash or publisher account.
        identifier: GlobalContractIdentifierView,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// A view function failed for a reason with no more specific variant.
    /// Missing code, missing methods and panics are already split out (from
    /// the structured `vm_error` or its legacy string form) as
    /// [`ContractNotDeployed`](Self::ContractNotDeployed),
    /// [`MethodNotFound`](Self::MethodNotFound) and
    /// [`ContractPanic`](Self::ContractPanic).
    #[error("contract execution failed on {contract_id}: {message}")]
    ContractExecution {
        /// The contract called. `unknown` only when parsing a raw response
        /// that omits it; `RpcClient::view_function` fills it in.
        contract_id: AccountId,
        /// The method called, when known.
        method_name: Option<String>,
        /// The node's description of the failure.
        message: String,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The requested method is not exported by the deployed contract.
    #[error("contract method not found: `{contract_id}::{method_name}`")]
    MethodNotFound {
        /// The contract called.
        contract_id: AccountId,
        /// The method that does not exist.
        method_name: String,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// A view method explicitly panicked during contract execution.
    ///
    /// Transaction function-call panics are reported in the transaction
    /// outcome instead; see [`crate::protocol::FunctionCallError`].
    #[error("contract panic: {message}")]
    ContractPanic {
        /// The panic message.
        message: String,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },

    /// The node doesn't have the block: it is unknown or was garbage-collected
    /// (`UNKNOWN_BLOCK`, `GARBAGE_COLLECTED_BLOCK`). Holds the block reference.
    ///
    /// Regular nodes keep about five epochs; ask an archival node for older
    /// blocks.
    #[error(
        "Block not found: {0}. It may have been garbage-collected. Try an archival node for blocks older than 5 epochs."
    )]
    BlockNotFound(String),

    /// The node doesn't have the chunk (`UNKNOWN_CHUNK`). Holds the chunk hash.
    #[error("Chunk not found: {0}. It may have been garbage-collected. Try an archival node.")]
    ChunkNotFound(String),

    /// The node can't find the epoch for the block (`UNKNOWN_EPOCH`). Holds
    /// the block reference.
    #[error(
        "Epoch not found for block: {0}. The block may be invalid or too old. Try an archival node."
    )]
    EpochNotFound(String),

    /// The shard ID doesn't exist (`INVALID_SHARD_ID`). Holds the shard ID.
    #[error("Invalid shard ID: {0}")]
    InvalidShardId(String),

    /// The node doesn't know the receipt (`UNKNOWN_RECEIPT`). Holds the
    /// receipt ID.
    #[error("Receipt not found: {0}")]
    ReceiptNotFound(String),

    /// `EXPERIMENTAL_receipt_to_tx` could not resolve the receipt for a reason
    /// other than the node not knowing it (that is
    /// [`ReceiptNotFound`](Self::ReceiptNotFound)). Asking the same node again
    /// gives the same answer.
    #[error("Receipt-to-transaction lookup failed: {0}")]
    ReceiptToTx(ReceiptToTxError),

    /// The node has no record of the transaction (`UNKNOWN_TRANSACTION`).
    ///
    /// nearcore answers this way only to a status request that does not wait
    /// (`wait_until: NONE`, near-kit's `Submitted`, the default for
    /// `Near::tx_status`). The transaction may not have reached the node yet:
    /// poll again later, or wait at a later level.
    #[error("Transaction not found: {0}")]
    TransactionNotFound(CryptoHash),

    /// The node does not track the transaction's shard, so it cannot answer
    /// (`DOES_NOT_TRACK_SHARD`). Use an RPC node that tracks every shard.
    #[error("Node does not track the transaction's shard")]
    ShardNotTracked,

    /// Structured transaction validation error, parsed from nearcore's
    /// `TxExecutionError::InvalidTxError`. Prefer matching on
    /// [`Error::InvalidTx`] instead — this variant only appears when using
    /// the low-level `rpc().send_tx()` API directly.
    #[error("Invalid transaction: {0}")]
    InvalidTx(crate::types::InvalidTxError),

    /// Fallback when the RPC returns `INVALID_TRANSACTION` but the structured
    /// error could not be deserialized into [`InvalidTxError`][crate::protocol::InvalidTxError].
    #[error("Invalid transaction: {message}")]
    InvalidTxUnparsed {
        /// The error message.
        message: String,
        /// The error's `data` field, which didn't parse as an
        /// `InvalidTxError`.
        details: Option<serde_json::Value>,
    },

    /// The node can't serve the shard right now (`UNAVAILABLE_SHARD`).
    /// Retryable.
    #[error("Shard unavailable: {0}")]
    ShardUnavailable(String),

    /// The node is still syncing (`NO_SYNCED_BLOCKS`, `NOT_SYNCED_YET`).
    /// Retryable.
    #[error("Node not synced: {0}")]
    NodeNotSynced(String),

    /// The node hit an internal error (`INTERNAL_ERROR`). Retryable.
    #[error("Internal server error: {0}")]
    InternalError(String),

    /// The node could not parse the request's parameters (`PARSE_ERROR`).
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
    Timeout {
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

    /// An unpaginated `view_access_key_list` request hit the node's cap
    /// (`TOO_MANY_ACCESS_KEYS`). Only `RpcClient::view_access_key_list_page`
    /// called without a `limit` or `after_key` can return it.
    #[error(
        "account {account_id} has more than {limit} access keys; use a paginated view_access_key_list request"
    )]
    TooManyAccessKeys {
        /// The account queried.
        account_id: AccountId,
        /// The most keys the node returns without pagination.
        limit: u32,
        /// Height of the block the node queried, if it reported one.
        block_height: Option<u64>,
        /// Hash of the block the node queried, if it reported one.
        block_hash: Option<CryptoHash>,
    },
}

/// Why a node returned `TIMEOUT_ERROR` for a transaction (nearcore 2.14+).
///
/// Carried by [`RpcError::Timeout`]. Mirrors nearcore's
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
///         RpcError::Timeout { cause: Some(TimeoutCause::Pending { .. }), .. } => {
///             "accepted, still waiting for finality: poll tx_status by hash"
///         }
///         RpcError::Timeout { cause: Some(TimeoutCause::NotObserved), .. } => {
///             "not on chain yet: re-sending the same signed transaction is safe"
///         }
///         RpcError::Timeout { .. } => "timed out",
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
    /// A cause near-kit does not recognize, or one whose payload failed to
    /// decode. Holds the raw `error.cause.info` JSON.
    Unknown(serde_json::Value),
}

/// Why `EXPERIMENTAL_receipt_to_tx` could not resolve a receipt.
///
/// Carried by [`RpcError::ReceiptToTx`]. Mirrors nearcore's
/// `RpcReceiptToTxError` causes other than `UNKNOWN_RECEIPT` and
/// `INTERNAL_ERROR`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ReceiptToTxError {
    /// The receipt's chain back to its transaction is longer than the node
    /// follows (`DEPTH_EXCEEDED`).
    #[error("depth limit {limit} exceeded when resolving receipt {receipt_id}")]
    DepthExceeded {
        /// The receipt being resolved.
        receipt_id: CryptoHash,
        /// The node's depth limit.
        limit: u32,
    },
    /// The node does not support the lookup (`UNSUPPORTED`).
    #[error("node does not support receipt-to-transaction lookup: {message}")]
    Unsupported {
        /// The node's explanation.
        message: String,
    },
    /// The node does not store execution outcomes (`OUTCOMES_NOT_STORED`).
    #[error("node does not store execution outcomes")]
    OutcomesNotStored,
    /// The requested search window is larger than the node allows
    /// (`WINDOW_TOO_LARGE`).
    #[error("requested window {requested} exceeds maximum {maximum}")]
    WindowTooLarge {
        /// The requested window, in blocks.
        requested: u64,
        /// The node's maximum, in blocks.
        maximum: u64,
    },
    /// The lookup hint is malformed (`MALFORMED_HINT`).
    #[error("malformed hint: {message}")]
    MalformedHint {
        /// The node's explanation.
        message: String,
    },
    /// The node scanned more outcomes than its budget allows
    /// (`BUDGET_EXCEEDED`).
    #[error("hint scan budget exceeded: {scanned} outcomes scanned, limit {limit}")]
    BudgetExceeded {
        /// Outcomes scanned.
        scanned: u64,
        /// The node's limit.
        limit: u64,
    },
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

/// Whether a `-32000` error cause names a condition that clears with time, so
/// that re-sending the same request to the same node can succeed.
///
/// From nearcore 2.14's `chain/jsonrpc-primitives/src/types/`:
///
/// - `NODE_IS_SYNCING`, `NO_NEW_BLOCKS` (`status.rs`): the node is catching up
///   or briefly stalled.
/// - `BUSY` (`indexer.rs`): the indexer endpoints' concurrency limit.
/// - `NOT_CONFIRMED`, `CHUNK_NOT_CERTIFIED` (`light_client.rs`): the outcome
///   or chunk isn't final yet; a later block confirms or certifies it.
///
/// `INTERNAL_ERROR`, `TIMEOUT_ERROR`, `UNAVAILABLE_SHARD`, `NOT_SYNCED_YET`
/// and `NO_SYNCED_BLOCKS` are transient too, but `RpcClient` maps them to
/// their own variants, so they never reach [`RpcError::Rpc`]; they are listed
/// for errors built by hand. Every other cause depends on the request, the
/// node's configuration or data it doesn't have (`REQUEST_ROUTED`, for
/// instance, routes the same transaction again), and is not retried.
fn is_transient_cause(cause: &str) -> bool {
    matches!(
        cause,
        "NODE_IS_SYNCING"
            | "NO_NEW_BLOCKS"
            | "BUSY"
            | "NOT_CONFIRMED"
            | "CHUNK_NOT_CERTIFIED"
            | "INTERNAL_ERROR"
            | "TIMEOUT_ERROR"
            | "UNAVAILABLE_SHARD"
            | "NOT_SYNCED_YET"
            | "NO_SYNCED_BLOCKS"
    )
}

impl RpcError {
    /// Whether re-sending the *same* request may succeed, which is what
    /// `RpcClient::request` checks between attempts: transport failures
    /// (including a connection the server reset or closed mid-exchange),
    /// timeouts, unsynced or unavailable nodes, `-32603`, and `-32000` with no
    /// cause or one that clears with time (`NODE_IS_SYNCING`, `BUSY`, ...).
    ///
    /// Of the [`InvalidTx`](Self::InvalidTx) rejections only `ShardCongested`
    /// and `ShardStuck` count. `InvalidNonce` doesn't: the same signed bytes
    /// can never pass, so the `SigningNear` transaction path re-signs with a
    /// fresh nonce instead.
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
            RpcError::Network { retryable, .. } => *retryable,
            RpcError::ShardUnavailable(_)
            | RpcError::NodeNotSynced(_)
            | RpcError::InternalError(_)
            | RpcError::Timeout { .. } => true,
            RpcError::InvalidTx(InvalidTxError::InvalidNonce { .. }) => false,
            RpcError::InvalidTx(e) => e.is_retryable(),
            // A server error is retried when the node names no cause (older
            // nodes and proxies) or a cause that clears with time; a cause
            // near-kit doesn't know may be deterministic.
            RpcError::Rpc {
                code: -32000,
                cause,
                ..
            } => cause.as_deref().is_none_or(is_transient_cause),
            RpcError::Rpc { code: -32603, .. } => true,
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
    pub(crate) fn invalid_tx(
        message: impl Into<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        if let Some(ref data) = details
            && let Some(invalid_tx) = Self::try_parse_invalid_tx(data)
        {
            return RpcError::InvalidTx(invalid_tx);
        }

        RpcError::InvalidTxUnparsed {
            message: message.into(),
            details,
        }
    }

    #[cfg_attr(not(feature = "rpc"), allow(dead_code))]
    /// Try to extract an `InvalidTxError` from the RPC error data JSON.
    ///
    /// The data field typically looks like:
    /// `{"TxExecutionError": {"InvalidTxError": {"InvalidNonce": {...}}}}`
    fn try_parse_invalid_tx(data: &serde_json::Value) -> Option<InvalidTxError> {
        use serde::Deserialize;
        // Nested under `TxExecutionError`, or at the top level (some RPC versions).
        [
            data.get("TxExecutionError")
                .and_then(|e| e.get("InvalidTxError")),
            data.get("InvalidTxError"),
        ]
        .into_iter()
        .flatten()
        .find_map(|v| InvalidTxError::deserialize(v).ok())
    }
}

/// Drops the request URL from the error: RPC providers often put an API key
/// in it (`?apikey=...`), and reqwest would print it in `Display` and `Debug`.
#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
impl From<reqwest::Error> for RpcError {
    fn from(error: reqwest::Error) -> Self {
        RpcError::Http(error.without_url())
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

impl RpcError {
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
            RpcError::BlockNotFound(_) => "BlockNotFound",
            RpcError::ChunkNotFound(_) => "ChunkNotFound",
            RpcError::EpochNotFound(_) => "EpochNotFound",
            RpcError::InvalidShardId(_) => "InvalidShardId",
            RpcError::ReceiptNotFound(_) => "ReceiptNotFound",
            RpcError::ReceiptToTx(_) => "ReceiptToTx",
            RpcError::TransactionNotFound(_) => "TransactionNotFound",
            RpcError::ShardNotTracked => "ShardNotTracked",
            RpcError::InvalidTx(_) => "InvalidTx",
            RpcError::InvalidTxUnparsed { .. } => "InvalidTxUnparsed",
            RpcError::ShardUnavailable(_) => "ShardUnavailable",
            RpcError::NodeNotSynced(_) => "NodeNotSynced",
            RpcError::InternalError(_) => "InternalError",
            RpcError::ParseError(_) => "ParseError",
            RpcError::Timeout { .. } => "Timeout",
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

/// The error type of near-kit's client operations.
///
/// A transaction whose actions ran and failed (a contract panic, a receiver
/// account that doesn't exist) is **not** an `Err`: the
/// network accepted and executed it, so it returns `Ok(outcome)` with
/// `outcome.is_failure()` set. Read the failure from the outcome.
///
/// An `Err` from a send means the transaction didn't run, or that near-kit
/// couldn't find out whether it did. The cases worth telling apart:
///
/// - [`InvalidTx`](Self::InvalidTx): the network rejected the transaction
///   before execution (bad nonce, not enough balance, expired). It didn't
///   run.
/// - [`Validation`](Self::Validation): a local check failed before anything
///   was signed or sent. So did [`Signing`](Self::Signing) and
///   [`Config`](Self::Config): nothing was sent.
/// - [`Rpc`](Self::Rpc): the request failed; see [`RpcError`] and
///   [`RpcError::is_retryable`]. Once the transaction was sent, the outcome
///   is unknown: after an [`RpcError::Timeout`] or a dropped connection it
///   may still run, or already have. Look it up by hash with
///   `Near::tx_status` before signing a replacement. near-kit's own retries
///   re-send the same signed transaction, so they can't run it twice.
///
#[cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#[cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
/// use near_kit::*;
/// use near_kit::protocol::Action;
///
/// # async fn example(alice: SigningNear) -> Result<(), Error> {
/// let transfer = alice
///     .transaction(account!("bob.testnet"))
///     .add_action(Action::transfer(NearToken::from_near(1)));
///
/// match transfer.await {
///     Ok(outcome) if outcome.is_success() => {
///         println!("Success! Hash: {}", outcome.transaction_hash());
///     }
///     Ok(outcome) => {
///         // Executed, but an action failed: inspect the outcome.
///         println!(
///             "Action failed: {:?}, gas used: {}",
///             outcome.failure_message(),
///             outcome.total_gas_used()
///         );
///     }
///     Err(Error::InvalidTx(e)) => {
///         // Rejected before execution: no receipt, no gas used.
///         println!("Transaction rejected: {e}");
///     }
///     Err(e) => return Err(e),
/// }
/// # Ok(())
/// # }
/// ```
///
/// The client-free error types of near-kit-primitives
/// ([`SignerError`](crate::signer::SignerError),
/// [`CodecError`](crate::transaction::CodecError),
/// [`OutcomeError`](crate::rpc::OutcomeError), and others) convert into this
/// type with `?`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The client or signer is misconfigured: a missing or malformed
    /// environment variable, an unreadable credentials file, or an empty or
    /// mixed-account `RotatingSigner` key set.
    #[error("Invalid configuration: {0}")]
    Config(String),

    /// An account ID did not parse.
    #[error(transparent)]
    ParseAccountId(#[from] ParseAccountIdError),

    /// A public or secret key did not parse.
    #[error(transparent)]
    ParseKey(#[from] ParseKeyError),

    /// The RPC request failed. Boxed to keep `Error` small.
    #[error(transparent)]
    Rpc(Box<RpcError>),

    /// Transaction was rejected before execution. No receipt was created,
    /// no nonce was incremented, no gas was consumed.
    ///
    /// This covers validation failures from both the RPC pre-check and the
    /// runtime execution layer — the caller never needs to distinguish them.
    #[error("Invalid transaction: {0}")]
    InvalidTx(Box<InvalidTxError>),

    /// A local check failed before anything was signed or sent: no
    /// actions, a state init or delegate action whose required receiver
    /// isn't the transaction's receiver, or a nested delegate action. Not a
    /// nearcore error; nothing reached the network.
    #[error("Refused before signing: {0}")]
    Validation(String),

    /// A transaction's outcome had no usable result: an action failed, it
    /// hasn't finished with a return value, or the RPC response carried no
    /// outcome. An outcome's `InvalidTx` and `Json` errors convert into
    /// [`Error::InvalidTx`] and [`Error::Json`] instead.
    #[error(transparent)]
    Outcome(OutcomeError),

    /// A signed transaction failed to decode from base64 or Borsh.
    #[error(transparent)]
    Decode(#[from] DecodeTransactionError),

    /// A signed delegate action failed to decode from base64 or Borsh
    /// (`SignedDelegateAction::from_base64` and the V2 equivalent).
    #[error(transparent)]
    DecodeDelegate(#[from] DecodeDelegateError),

    /// The signer could not sign, or could not be built (for example an
    /// invalid seed phrase).
    #[error("Signing failed: {0}")]
    Signing(#[source] SignerError),

    /// Loading a key from a key store failed.
    #[error(transparent)]
    KeyStore(#[from] KeyStoreError),

    /// JSON encoding or decoding failed: call arguments, a view's return
    /// value, or a transaction's JSON result.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Borsh encoding or decoding of call arguments or a return value
    /// failed. Holds the error message.
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
            other => Error::Validation(other.to_string()),
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
            other => Error::Validation(other.to_string()),
        }
    }
}

impl From<OutcomeError> for Error {
    fn from(err: OutcomeError) -> Self {
        match err {
            OutcomeError::InvalidTx(e) => Error::InvalidTx(e),
            OutcomeError::Json(e) => Error::Json(e),
            other => Error::Outcome(other),
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
    /// use near_kit::rpc::RpcError;
    ///
    /// match near.view::<u64>(account!("counter.testnet"), "get_count").await.map_err(near_kit::Error::from) {
    ///     Ok(count) => println!("{count}"),
    ///     Err(e) if matches!(e.as_rpc(), Some(RpcError::ContractNotDeployed { .. })) => {
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

    #[test]
    fn test_rpc_error_is_retryable() {
        use crate::types::InvalidTxError;

        // Retryable errors
        assert!(RpcError::ShardUnavailable("shard 0".to_string()).is_retryable());
        assert!(RpcError::NodeNotSynced("syncing".to_string()).is_retryable());
        assert!(RpcError::InternalError("db error".to_string()).is_retryable());
        assert!(
            RpcError::Timeout {
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
        let server_error = |cause: Option<&str>| RpcError::Rpc {
            code: -32000,
            message: "server error".to_string(),
            data: None,
            cause: cause.map(str::to_string),
        };
        assert!(server_error(None).is_retryable());
        assert!(server_error(Some("NODE_IS_SYNCING")).is_retryable());
        assert!(server_error(Some("NOT_CONFIRMED")).is_retryable());
        // A cause near-kit doesn't know isn't retried.
        assert!(!server_error(Some("FUTURE_HANDLER_ERROR")).is_retryable());
        assert!(!server_error(Some("SHARD_NOT_TRACKED")).is_retryable());

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
        assert!(!RpcError::BlockNotFound("12345".to_string()).is_retryable());
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
            !RpcError::InvalidTxUnparsed {
                message: "invalid".to_string(),
                details: None,
            }
            .is_retryable()
        );
    }

    #[test]
    fn test_rpc_error_view_call_block_context() {
        let account_id: AccountId = "contract.near".parse().unwrap();
        let block_hash = CryptoHash::hash(b"block");
        let method_not_found = RpcError::MethodNotFound {
            contract_id: account_id,
            method_name: "missing".to_string(),
            block_height: Some(42),
            block_hash: Some(block_hash),
        };
        assert_eq!(method_not_found.block_height(), Some(42));
        assert_eq!(method_not_found.block_hash(), Some(block_hash));

        let panic = RpcError::ContractPanic {
            message: "boom".to_string(),
            block_height: None,
            block_hash: None,
        };
        assert_eq!(panic.block_height(), None);
        assert_eq!(panic.block_hash(), None);
        assert_eq!(
            RpcError::InvalidResponse("missing result".to_string()).block_height(),
            None
        );
        assert_eq!(
            RpcError::InvalidResponse("missing result".to_string()).block_hash(),
            None
        );
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
    fn test_rpc_error_timeout_display() {
        let err = RpcError::Timeout {
            message: "request timed out".to_string(),
            transaction_hash: Some(CryptoHash::ZERO),
            cause: None,
        };
        assert_eq!(err.to_string(), "Request timeout: request timed out");

        let err = RpcError::Timeout {
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
        let rpc_err = RpcError::InvalidResponse("missing result".to_string());
        let err: Error = rpc_err.into();
        assert!(matches!(err, Error::Rpc(_)));
    }

    #[test]
    fn test_error_from_signer_error() {
        let signer_err = SignerError::InvalidSeedPhrase;
        let err: Error = signer_err.into();
        assert!(matches!(err, Error::Signing(_)));
    }

    // How each layer error converts into `Error` with `?`.

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
    fn test_error_from_decode_delegate_error() {
        fn decode(s: &str) -> Result<crate::protocol::SignedDelegateAction, Error> {
            Ok(crate::protocol::SignedDelegateAction::from_base64(s)?)
        }
        let err = decode("!!").unwrap_err();
        assert!(
            matches!(&err, Error::DecodeDelegate(DecodeDelegateError::Base64(_))),
            "{err:?}"
        );
        assert!(
            err.to_string().starts_with("base64 decode error: "),
            "{err}"
        );

        // Valid base64, not a borsh-encoded signed delegate action.
        let err = decode("AQID").unwrap_err();
        assert!(
            matches!(&err, Error::DecodeDelegate(DecodeDelegateError::Borsh(_))),
            "{err:?}"
        );

        let err: Error = crate::protocol::VersionedSignedDelegateAction::from_base64("!!")
            .unwrap_err()
            .into();
        assert!(matches!(err, Error::DecodeDelegate(_)), "{err:?}");

        // `from_bytes` returns the same error type, so `?` converts it too.
        let err: Error = crate::protocol::SignedDelegateAction::from_bytes(&[1, 2, 3])
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::DecodeDelegate(DecodeDelegateError::Borsh(_))),
            "{err:?}"
        );
        let err: Error = crate::protocol::VersionedSignedDelegateAction::from_bytes(&[1, 2, 3])
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::DecodeDelegate(DecodeDelegateError::Borsh(_))),
            "{err:?}"
        );
    }

    #[test]
    fn test_error_from_decode_transaction_error() {
        let err: Error = crate::protocol::SignedTransaction::from_base64("!!")
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::Decode(DecodeTransactionError::Base64(_)))
                && err.to_string().starts_with("Invalid base64: "),
            "{err:?}"
        );

        let err: Error = crate::protocol::SignedTransaction::from_bytes(&[1, 2, 3])
            .unwrap_err()
            .into();
        assert!(
            matches!(&err, Error::Decode(DecodeTransactionError::Borsh(_)))
                && err
                    .to_string()
                    .starts_with("Failed to deserialize signed transaction: "),
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
            matches!(&err, Error::Outcome(OutcomeError::NotSuccess(_)))
                && err.to_string() == "Transaction status is Started, expected SuccessValue",
            "{err:?}"
        );

        let err: Error = OutcomeError::MissingOutcome {
            transaction_hash: CryptoHash::default(),
            wait_level: "Final",
        }
        .into();
        assert!(
            matches!(&err, Error::Outcome(OutcomeError::MissingOutcome { .. }))
                && err.to_string().ends_with(" at wait level Final"),
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
        assert!(matches!(err.as_rpc(), Some(RpcError::ContractPanic { .. })));

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
        assert!(matches!(err.as_rpc(), Some(RpcError::ContractPanic { .. })));
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

    /// A local refusal and a node's rejection read differently in logs.
    #[test]
    fn validation_and_invalid_tx_display_apart() {
        assert_eq!(
            Error::Validation("transaction must have at least one action".to_string()).to_string(),
            "Refused before signing: transaction must have at least one action"
        );
        assert_eq!(
            Error::InvalidTx(Box::new(InvalidTxError::Expired)).to_string(),
            "Invalid transaction: transaction has expired"
        );
    }
}
