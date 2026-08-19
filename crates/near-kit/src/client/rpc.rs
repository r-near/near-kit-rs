//! Low-level JSON-RPC client for NEAR.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::transport::RpcTransport;
// The module itself is only referenced for `default_transport`, which doesn't
// exist on WASI builds without the `wasi-http` feature (no built-in transport).
#[cfg(any(
    not(all(target_arch = "wasm32", target_os = "wasi")),
    all(feature = "wasi-http", target_env = "p2")
))]
use super::transport;
use crate::error::RpcError;
use crate::trace;
use crate::types::rpc::RawTransactionResponse;
use crate::types::{
    AccessKeyListView, AccessKeyView, AccountId, AccountView, BlockEffects, BlockReference,
    BlockView, CompilationError, ContractCodeView, CryptoHash, EpochValidatorInfo,
    FunctionCallError, GasKeyNoncesView, GasPrice, GlobalContractId, GlobalContractIdentifierView,
    HostError, MaintenanceWindow, MethodResolveError, PublicKey, ReceiptToTxResponse,
    SignedTransaction, StateItem, StatusResponse, TxExecutionStatus, ViewFunctionResult,
    ViewStateResult,
};

/// Platform-appropriate async sleep, used for retry backoff.
///
/// - Native (non-wasm): `tokio::time::sleep`.
/// - WASI: `std::thread::sleep`. A WASI guest is single-threaded with no tokio
///   runtime, so `tokio::time::sleep` would panic ("no reactor running") —
///   and blocking the one thread is fine, since the whole guest is already
///   blocked on this future (the wasi:http transport is blocking too).
/// - `wasm32-unknown-unknown`: no OS timers — use the JS host's via `gloo-timers`.
async fn async_sleep(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(duration).await;
    }

    #[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
    {
        std::thread::sleep(duration);
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        gloo_timers::future::sleep(duration).await;
    }
}

/// Network configuration presets.
pub struct NetworkConfig {
    /// The RPC URL for this network.
    pub rpc_url: &'static str,
    /// The network identifier (e.g., "mainnet", "testnet").
    /// Reserved for future use in transaction signing.
    #[allow(dead_code)]
    pub network_id: &'static str,
}

/// Mainnet configuration.
pub const MAINNET: NetworkConfig = NetworkConfig {
    rpc_url: "https://free.rpc.fastnear.com",
    network_id: "mainnet",
};

/// Testnet configuration.
pub const TESTNET: NetworkConfig = NetworkConfig {
    rpc_url: "https://test.rpc.fastnear.com",
    network_id: "testnet",
};

/// Retry configuration for RPC calls.
///
/// Governs how [`RpcClient::call`] retries transient failures (transport
/// errors, timeouts, 5xx / server-side errors — anything where
/// [`RpcError::is_retryable`] is `true`). Delays grow exponentially from
/// `initial_delay_ms`, capped at `max_delay_ms`.
///
/// A transaction rejected with `InvalidNonce` is never retried here —
/// re-sending the same signed bytes can't change the outcome. Nonce refresh
/// and re-signing happen one layer up, in the `Near::send*` transaction path
/// (see `NearBuilder::max_nonce_retries`).
///
/// Use [`RetryConfig::none()`] to disable retries entirely, e.g. when the
/// caller runs its own retry loop.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retries after the first attempt (`0` = one attempt).
    pub max_retries: u32,
    /// Delay before the first retry, in milliseconds. Doubles on each
    /// subsequent retry.
    pub initial_delay_ms: u64,
    /// Upper bound on the delay between retries, in milliseconds.
    pub max_delay_ms: u64,
}

impl RetryConfig {
    /// Disable retries: every call makes exactly one attempt and returns the
    /// first error, retryable or not.
    ///
    /// Useful when the caller owns its retry policy (a CLI with its own
    /// "retry?" prompt, a relayer with its own backoff), so near-kit's
    /// built-in loop doesn't add hidden delay on top.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::{Near, RetryConfig};
    ///
    /// let near = Near::testnet()
    ///     .retry_config(RetryConfig::none())
    ///     .build();
    /// ```
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }
}

impl Default for RetryConfig {
    /// Three retries (four attempts total) with exponential backoff starting
    /// at 500 ms and capped at 5 s: 500 ms, 1 s, 2 s.
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 5000,
        }
    }
}

/// JSON-RPC request structure.
#[derive(Serialize)]
struct JsonRpcRequest<'a, P: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: P,
}

/// JSON-RPC response structure.
///
/// The `result` field is deserialized as raw JSON first, then parsed into `T`
/// only after confirming no error is present. This avoids deserialization
/// failures when the RPC returns an error with a partial/unexpected `result`.
#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error structure.
/// NEAR RPC returns structured errors with name/cause/info pattern.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    cause: Option<ErrorCause>,
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
}

/// Structured error cause from NEAR RPC.
#[derive(Debug, Deserialize)]
struct ErrorCause {
    name: String,
    #[serde(default)]
    info: Option<serde_json::Value>,
}

/// Extract the block context included in view RPC error payloads.
fn parse_error_block_context(
    info: Option<&serde_json::Value>,
) -> (Option<u64>, Option<CryptoHash>) {
    let block_height = info
        .and_then(|value| value.get("block_height"))
        .and_then(serde_json::Value::as_u64);
    let block_hash = info
        .and_then(|value| value.get("block_hash"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse().ok());
    (block_height, block_hash)
}

/// Extract nearcore's structured `FunctionCallError` from a handler error.
///
/// Current nodes use `info.vm_error`; the initial structured endpoint shape
/// used `info.error`. Accept both so providers on either nearcore generation
/// get the same typed classification.
fn parse_function_call_error(info: Option<&serde_json::Value>) -> Option<FunctionCallError> {
    let info = info?;

    ["vm_error", "error"].into_iter().find_map(|key| {
        let value = info.get(key)?;
        // The legacy `query` endpoint also has a `vm_error` string. It is not
        // structured and should not mask a structured `error` sibling.
        if !value.is_object() {
            return None;
        }
        let value = value.get("FunctionCallError").unwrap_or(value);
        match serde_json::from_value(value.clone()).ok()? {
            FunctionCallError::Unknown(_) => None,
            error => Some(error),
        }
    })
}

/// Map the function-call cases callers need to branch on independently.
fn classify_function_call_error(
    error: &FunctionCallError,
    contract_id: &AccountId,
    method_name: Option<&str>,
    block_height: Option<u64>,
    block_hash: Option<CryptoHash>,
) -> Option<RpcError> {
    match error {
        FunctionCallError::CompilationError(CompilationError::CodeDoesNotExist { account_id }) => {
            Some(RpcError::ContractNotDeployed {
                account_id: account_id.clone(),
                block_height,
                block_hash,
            })
        }
        FunctionCallError::MethodResolveError(MethodResolveError::MethodNotFound) => {
            Some(RpcError::MethodNotFound {
                contract_id: contract_id.clone(),
                method_name: method_name.unwrap_or("unknown").to_string(),
                block_height,
                block_hash,
            })
        }
        FunctionCallError::HostError(HostError::GuestPanic { panic_msg }) => {
            Some(RpcError::ContractPanic {
                message: panic_msg.clone(),
                block_height,
                block_hash,
            })
        }
        FunctionCallError::ExecutionError(message) => {
            extract_contract_panic_message(message).map(|message| RpcError::ContractPanic {
                message,
                block_height,
                block_hash,
            })
        }
        _ => None,
    }
}

/// Classify the string form of a `FunctionCallError` when no usable structured
/// `vm_error` accompanies it.
///
/// Older nodes and some providers only report the `Debug` rendering of the
/// error, wrapped in `Function call returned an error: ...` (RPC `data`) or
/// `wasm execution failed with error: ...` (legacy `query` `vm_error`
/// string). Map the same cases as [`classify_function_call_error`] so callers
/// can rely on the typed variants regardless of which shape the provider
/// returns. Anything unrecognized stays `None` so the caller keeps the raw
/// message in [`RpcError::ContractExecution`].
fn classify_legacy_function_call_error(
    message: &str,
    contract_id: &AccountId,
    method_name: Option<&str>,
    block_height: Option<u64>,
    block_hash: Option<CryptoHash>,
) -> Option<RpcError> {
    const ENVELOPES: [&str; 2] = [
        "Function call returned an error:",
        "wasm execution failed with error:",
    ];

    let error = ENVELOPES.iter().fold(message.trim(), |rest, prefix| {
        rest.strip_prefix(prefix).map_or(rest, str::trim_start)
    });

    // Anchor on the outer variant. String-bearing variants (`LinkError`,
    // `ExecutionError`, ...) carry free text that could embed any of these
    // tokens, so only a match at the start of the rendering counts.
    if let Some(rest) = error.strip_prefix("ExecutionError(") {
        // `ExecutionError("Smart contract panicked: ...")`
        let inner = parse_debug_str(rest)
            .unwrap_or_else(|| rest.trim_end().trim_end_matches(')').to_string());
        return extract_contract_panic_message(&inner).map(|message| RpcError::ContractPanic {
            message,
            block_height,
            block_hash,
        });
    }
    if let Some(rest) = error.strip_prefix("HostError(GuestPanic") {
        // `HostError(GuestPanic { panic_msg: "..." })`
        let message = rest
            .split_once("panic_msg:")
            .and_then(|(_, rest)| parse_debug_str(rest.trim_start()))
            .unwrap_or_else(|| error.to_string());
        return Some(RpcError::ContractPanic {
            message,
            block_height,
            block_hash,
        });
    }
    if let Some(rest) = error.strip_prefix("CompilationError(CodeDoesNotExist") {
        // `CompilationError(CodeDoesNotExist { account_id: AccountId("x.near") })`
        let account_id = rest
            .split_once("account_id:")
            .and_then(|(_, rest)| rest.find('"').map(|index| &rest[index..]))
            .and_then(parse_debug_str)
            .and_then(|account_id| account_id.parse().ok())
            .unwrap_or_else(|| contract_id.clone());
        return Some(RpcError::ContractNotDeployed {
            account_id,
            block_height,
            block_hash,
        });
    }
    if error.starts_with("MethodResolveError(MethodNotFound)") {
        return Some(RpcError::MethodNotFound {
            contract_id: contract_id.clone(),
            method_name: method_name.unwrap_or("unknown").to_string(),
            block_height,
            block_hash,
        });
    }

    // `Display` rendering of a guest panic.
    extract_contract_panic_message(error).map(|message| RpcError::ContractPanic {
        message,
        block_height,
        block_hash,
    })
}

/// Parse a Rust `Debug`-formatted string literal (`"..."`, with escapes) at the
/// start of `input`, returning its unescaped contents.
fn parse_debug_str(input: &str) -> Option<String> {
    let mut chars = input.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                'u' => {
                    // `\u{XXXX}`
                    if chars.next()? != '{' {
                        return None;
                    }
                    let hex: String = chars.by_ref().take_while(|c| *c != '}').collect();
                    out.push(
                        u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)?,
                    );
                }
                // `\"`, `\\`, `\'`
                escaped => out.push(escaped),
            },
            c => out.push(c),
        }
    }
}

/// nearcore collapses current execution failures to strings, but retains a
/// stable prefix for explicit guest panics.
fn extract_contract_panic_message(message: &str) -> Option<String> {
    const PREFIX: &str = "Smart contract panicked";

    let candidate_prefix = message.get(..PREFIX.len())?;
    if !candidate_prefix.eq_ignore_ascii_case(PREFIX) {
        return None;
    }

    match &message[PREFIX.len()..] {
        "" => Some(message.to_string()),
        rest if rest.starts_with(':') => {
            let panic_message = rest[1..].trim_start();
            Some(if panic_message.is_empty() {
                message.to_string()
            } else {
                panic_message.to_string()
            })
        }
        _ => None,
    }
}

/// Response from EXPERIMENTAL_call_function.
/// Errors are returned through the JSON-RPC error envelope, so no `error`
/// field is needed here.
#[derive(Debug, Deserialize)]
struct CallFunctionResponse {
    result: Vec<u8>,
    #[serde(default)]
    logs: Vec<String>,
    block_height: u64,
    block_hash: CryptoHash,
}

/// Low-level JSON-RPC client for NEAR.
pub struct RpcClient {
    url: String,
    transport: Arc<dyn RpcTransport>,
    retry_config: RetryConfig,
    request_id: AtomicU64,
}

impl RpcClient {
    /// Page size [`RpcClient::view_state_all`] uses when asked for `0`.
    ///
    /// This is nearcore's `MAX_VIEW_STATE_PAGE_ITEMS`; the node also caps every
    /// page at ~50 KB of state, so the exact value rarely matters. What does
    /// matter is that a `limit` is always sent: nearcore only treats a
    /// `view_state` query as paginated when `limit` or `after_key` is present,
    /// and only paginated queries are exempt from the `TOO_LARGE_CONTRACT_STATE`
    /// size gate.
    pub const DEFAULT_VIEW_STATE_PAGE_SIZE: u32 = 10_000;

    /// Create a new RPC client with the given URL.
    ///
    /// Only exists where a built-in transport does (every target except WASI
    /// without the `wasi-http` feature) — otherwise construct via
    /// [`with_transport_and_retry_config`](Self::with_transport_and_retry_config).
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "wasi")),
        all(feature = "wasi-http", target_env = "p2")
    ))]
    pub fn new(url: impl Into<String>) -> Self {
        Self::with_transport_and_retry_config(
            url,
            transport::default_transport(),
            RetryConfig::default(),
        )
    }

    /// Create a new RPC client with custom retry configuration.
    ///
    /// Only exists where a built-in transport does — see [`RpcClient::new`].
    #[cfg(any(
        not(all(target_arch = "wasm32", target_os = "wasi")),
        all(feature = "wasi-http", target_env = "p2")
    ))]
    pub fn with_retry_config(url: impl Into<String>, retry_config: RetryConfig) -> Self {
        Self::with_transport_and_retry_config(url, transport::default_transport(), retry_config)
    }

    /// Create a new RPC client with a custom [`RpcTransport`] and retry
    /// configuration.
    ///
    /// This is the low-level injection point for platforms whose HTTP stack
    /// near-kit doesn't know about. Most callers should use
    /// [`NearBuilder::transport`](super::NearBuilder::transport) instead.
    pub fn with_transport_and_retry_config(
        url: impl Into<String>,
        transport: Arc<dyn RpcTransport>,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            url: url.into(),
            transport,
            retry_config,
            request_id: AtomicU64::new(0),
        }
    }

    /// Get the RPC URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Make a raw RPC call with retries.
    ///
    /// Transient failures ([`RpcError::is_retryable`]) are retried according
    /// to the client's [`RetryConfig`]. An `InvalidNonce` rejection is
    /// terminal here: the node has rejected this exact signed payload, so it
    /// is returned after a single attempt without retrying (the transaction
    /// layer re-signs with a fresh nonce instead).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, params), fields(rpc.method = method, rpc.url = %sanitize_url(&self.url))))]
    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, RpcError> {
        let total_attempts = self.retry_config.max_retries + 1;

        for attempt in 0..total_attempts {
            let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);

            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: request_id,
                method,
                params: &params,
            };

            match self.try_call::<R>(&request).await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() && attempt < total_attempts - 1 => {
                    let delay = std::cmp::min(
                        self.retry_config.initial_delay_ms * 2u64.pow(attempt),
                        self.retry_config.max_delay_ms,
                    );
                    // DEBUG, not WARN: the retry is routine and the caller
                    // sees the final outcome either way. Retryable variants
                    // are never enriched downstream, so `Display` is accurate.
                    trace::debug!(
                        attempt = attempt + 1,
                        max_attempts = total_attempts,
                        delay_ms = delay,
                        error = %e,
                        "RPC request failed, retrying"
                    );
                    async_sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                Err(e) => {
                    // DEBUG, not ERROR: the error is returned to the caller,
                    // who decides whether it is worth reporting. Only the
                    // variant is logged — the typed helpers may still patch
                    // caller-known context (contract, method, ...) into it,
                    // so its `Display` text can be misleading here.
                    trace::debug!(error.kind = e.variant_name(), "RPC request failed");
                    return Err(e);
                }
            }
        }

        unreachable!("all loop iterations return")
    }

    /// Single attempt to make an RPC call.
    async fn try_call<R: DeserializeOwned>(
        &self,
        request: &JsonRpcRequest<'_, impl Serialize>,
    ) -> Result<R, RpcError> {
        // Gated on the feature (not routed through `crate::trace`) because the
        // no-op macros would leave `json` unused.
        #[cfg(feature = "tracing")]
        if tracing::enabled!(tracing::Level::TRACE)
            && let Ok(json) = serde_json::to_string(request)
        {
            tracing::trace!(payload = %json, "RPC request");
        }

        let request_body = serde_json::to_vec(request).map_err(RpcError::Json)?;
        let response = self.transport.post_json(&self.url, request_body).await?;

        let status = response.status;
        // Lossy decode matches what reqwest's `text()` did here before the
        // transport seam: the RPC's JSON bodies are UTF-8, and anything mangled
        // by a proxy fails in the JSON parse below with the payload visible.
        let body = String::from_utf8_lossy(&response.body);

        trace::trace!(payload = %body, "RPC response");

        if !(200..300).contains(&status) {
            // nearcore returns non-2xx (e.g. 422 UNKNOWN_BLOCK, 408 TIMEOUT_ERROR) with
            // a well-formed JSON-RPC error body — try to decode that first so callers
            // get typed variants instead of an opaque Network error. Falls back to the
            // original Network error for non-JSON bodies (HTML error pages, etc.).
            if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(&body)
                && let Some(error) = parsed.error
            {
                let parsed_err = self.parse_rpc_error(&error);
                return Err(preserve_http_retry_classification(
                    parsed_err, status, &body,
                ));
            }
            let retryable = is_retryable_status(status);
            return Err(RpcError::network(
                format!("HTTP {}: {}", status, body),
                Some(status),
                retryable,
            ));
        }

        let rpc_response: JsonRpcResponse = serde_json::from_str(&body).map_err(RpcError::Json)?;

        if let Some(error) = rpc_response.error {
            return Err(self.parse_rpc_error(&error));
        }

        let result_value = rpc_response
            .result
            .ok_or_else(|| RpcError::InvalidResponse("Missing result in response".to_string()))?;

        // NEAR's `query` endpoint sometimes returns errors inside the result
        // object (with an "error" field) instead of in the JSON-RPC error
        // envelope. Only check for this on the `query` method to avoid
        // misclassifying legitimate results from other methods.
        if request.method == "query"
            && let Some(error_str) = result_value.get("error").and_then(|e| e.as_str())
        {
            let synthetic = JsonRpcError {
                // Use -32600 (Invalid Request) rather than -32000 (Server Error)
                // so deterministic failures don't get retried.
                code: -32600,
                message: error_str.to_string(),
                data: Some(serde_json::Value::String(error_str.to_string())),
                cause: None,
                name: None,
            };
            return Err(self.parse_rpc_error(&synthetic));
        }

        serde_json::from_value(result_value).map_err(RpcError::Json)
    }

    /// Parse an RPC error into a specific error type.
    fn parse_rpc_error(&self, error: &JsonRpcError) -> RpcError {
        // First, check the direct cause field (NEAR RPC structured errors)
        if let Some(cause) = &error.cause {
            let cause_name = cause.name.as_str();
            let info = cause.info.as_ref();
            let data = &error.data;
            let (block_height, block_hash) = parse_error_block_context(info);

            match cause_name {
                "UNKNOWN_ACCOUNT" => {
                    if let Some(account_id) = info
                        .and_then(|i| i.get("requested_account_id"))
                        .and_then(|a| a.as_str())
                        && let Ok(account_id) = account_id.parse()
                    {
                        return RpcError::AccountNotFound {
                            account_id,
                            block_height,
                            block_hash,
                        };
                    }
                }
                "INVALID_ACCOUNT" => {
                    let account_id = info
                        .and_then(|i| i.get("requested_account_id"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("unknown");
                    return RpcError::InvalidAccount {
                        account_id: account_id.to_string(),
                        block_height,
                        block_hash,
                    };
                }
                "UNKNOWN_ACCESS_KEY" | "UNKNOWN_GAS_KEY" => {
                    if let Some(public_key) = info
                        .and_then(|i| i.get("public_key"))
                        .and_then(|k| k.as_str())
                        .and_then(|k| k.parse().ok())
                    {
                        // nearcore's payload only carries the public key
                        // (`requested_account_id` is a legacy `query` extra);
                        // the typed helpers patch in the caller-known account.
                        // Fall back to "unknown".
                        let account_id = info
                            .and_then(|i| i.get("requested_account_id"))
                            .and_then(|a| a.as_str())
                            .and_then(|a| a.parse().ok())
                            .unwrap_or_else(|| "unknown".parse().unwrap());
                        if cause_name == "UNKNOWN_GAS_KEY" {
                            return RpcError::GasKeyNotFound {
                                account_id,
                                public_key,
                                block_height,
                                block_hash,
                            };
                        }
                        return RpcError::AccessKeyNotFound {
                            account_id,
                            public_key,
                            block_height,
                            block_hash,
                        };
                    }
                }
                "UNKNOWN_BLOCK" => {
                    let block_ref = data
                        .as_ref()
                        .and_then(|d| d.as_str())
                        .unwrap_or(&error.message);
                    return RpcError::UnknownBlock(block_ref.to_string());
                }
                "UNKNOWN_CHUNK" => {
                    let chunk_ref = info
                        .and_then(|i| i.get("chunk_hash"))
                        .and_then(|c| c.as_str())
                        .unwrap_or(&error.message);
                    return RpcError::UnknownChunk(chunk_ref.to_string());
                }
                "UNKNOWN_EPOCH" => {
                    let block_ref = data
                        .as_ref()
                        .and_then(|d| d.as_str())
                        .unwrap_or(&error.message);
                    return RpcError::UnknownEpoch(block_ref.to_string());
                }
                "UNKNOWN_RECEIPT" => {
                    let receipt_id = info
                        .and_then(|i| i.get("receipt_id"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");
                    return RpcError::UnknownReceipt(receipt_id.to_string());
                }
                "NO_CONTRACT_CODE" => {
                    let account_id = info
                        .and_then(|i| {
                            i.get("contract_account_id")
                                .or_else(|| i.get("account_id"))
                                .or_else(|| i.get("contract_id"))
                        })
                        .and_then(|a| a.as_str())
                        .unwrap_or("unknown");
                    if let Ok(account_id) = account_id.parse() {
                        return RpcError::ContractNotDeployed {
                            account_id,
                            block_height,
                            block_hash,
                        };
                    }
                }
                "NO_GLOBAL_CONTRACT_CODE" => {
                    // The cause name is authoritative; the identifier parse is
                    // best-effort so an unexpected shape can't demote this to a
                    // generic error. `view_global_contract_code` patches in the
                    // caller-known identifier anyway.
                    let identifier = info
                        .and_then(|i| i.get("identifier"))
                        .and_then(|v| {
                            serde_json::from_value::<GlobalContractIdentifierView>(v.clone()).ok()
                        })
                        .unwrap_or_else(|| {
                            GlobalContractIdentifierView::AccountId("unknown".parse().unwrap())
                        });
                    return RpcError::GlobalContractNotFound {
                        identifier,
                        block_height,
                        block_hash,
                    };
                }
                "TOO_LARGE_CONTRACT_STATE" => {
                    // nearcore's `RpcQueryError::TooLargeContractState` puts the
                    // account under `contract_account_id`; the other keys are
                    // lenient fallbacks. If none parses, fall through to the
                    // generic error rather than inventing an account id.
                    if let Some(account_id) = info
                        .and_then(|i| {
                            i.get("contract_account_id")
                                .or_else(|| i.get("account_id"))
                                .or_else(|| i.get("contract_id"))
                        })
                        .and_then(|a| a.as_str())
                        .and_then(|a| a.parse().ok())
                    {
                        return RpcError::ContractStateTooLarge {
                            account_id,
                            block_height,
                            block_hash,
                        };
                    }
                }
                "CONTRACT_EXECUTION_ERROR" => {
                    // Legacy `query` includes contract_id/method_name;
                    // EXPERIMENTAL_call_function does not (the caller
                    // already knows them). Fall back to "unknown".
                    let contract_id = info
                        .and_then(|i| i.get("contract_id"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown")
                        .parse()
                        .unwrap_or_else(|_| "unknown".parse().unwrap());
                    let method_name = info
                        .and_then(|i| i.get("method_name"))
                        .and_then(|m| m.as_str())
                        .map(String::from);
                    let function_call_error = parse_function_call_error(info);
                    if let Some(specific_error) = function_call_error.as_ref().and_then(|error| {
                        classify_function_call_error(
                            error,
                            &contract_id,
                            method_name.as_deref(),
                            block_height,
                            block_hash,
                        )
                    }) {
                        return specific_error;
                    }

                    // Legacy shape: no usable structured `vm_error`, only the
                    // Debug rendering of the error in `data` (or in the legacy
                    // `query` endpoint's `vm_error` string).
                    let legacy_messages = [
                        data.as_ref().and_then(|d| d.as_str()),
                        info.and_then(|i| i.get("vm_error"))
                            .and_then(|v| v.as_str()),
                    ];
                    if let Some(specific_error) =
                        legacy_messages.into_iter().flatten().find_map(|message| {
                            classify_legacy_function_call_error(
                                message,
                                &contract_id,
                                method_name.as_deref(),
                                block_height,
                                block_hash,
                            )
                        })
                    {
                        return specific_error;
                    }

                    let message = data
                        .as_ref()
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| function_call_error.as_ref().map(ToString::to_string))
                        .or_else(|| {
                            // EXPERIMENTAL endpoint: use vm_error as fallback
                            // when data isn't a string
                            info.and_then(|i| i.get("vm_error")).map(|v| v.to_string())
                        })
                        .unwrap_or_else(|| error.message.clone());
                    return RpcError::ContractExecution {
                        contract_id,
                        method_name,
                        message,
                        block_height,
                        block_hash,
                    };
                }
                "UNAVAILABLE_SHARD" => {
                    return RpcError::ShardUnavailable(error.message.clone());
                }
                "NO_SYNCED_BLOCKS" | "NOT_SYNCED_YET" => {
                    return RpcError::NodeNotSynced(error.message.clone());
                }
                "INVALID_SHARD_ID" => {
                    let shard_id = info
                        .and_then(|i| i.get("shard_id"))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    return RpcError::InvalidShardId(shard_id);
                }
                "INVALID_TRANSACTION" => {
                    return RpcError::invalid_transaction(&error.message, data.clone());
                }
                "TIMEOUT_ERROR" => {
                    let tx_hash = info
                        .and_then(|i| i.get("transaction_hash"))
                        .and_then(|h| h.as_str())
                        .map(String::from);
                    return RpcError::RequestTimeout {
                        message: error.message.clone(),
                        transaction_hash: tx_hash,
                    };
                }
                "PARSE_ERROR" => {
                    return RpcError::ParseError(error.message.clone());
                }
                "INTERNAL_ERROR" => {
                    return RpcError::InternalError(error.message.clone());
                }
                _ => {}
            }
        }

        // Fallback: check for string error messages in data field
        if let Some(data) = &error.data
            && let Some(error_str) = data.as_str()
            && error_str.contains("does not exist")
        {
            // Try to extract account ID from error message
            // Format: "account X does not exist while viewing"
            if let Some(start) = error_str.strip_prefix("account ")
                && let Some(account_str) = start.split_whitespace().next()
                && let Ok(account_id) = account_str.parse()
            {
                return RpcError::AccountNotFound {
                    account_id,
                    block_height: None,
                    block_hash: None,
                };
            }
        }

        RpcError::Rpc {
            code: error.code,
            message: error.message.clone(),
            data: error.data.clone(),
        }
    }

    // ========================================================================
    // High-level RPC methods
    // ========================================================================

    /// View account information.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(%account_id)))]
    pub async fn view_account(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<AccountView, RpcError> {
        let mut params = serde_json::json!({
            "account_id": account_id.to_string(),
        });
        self.merge_block_reference(&mut params, &block);
        self.call("EXPERIMENTAL_view_account", params).await
    }

    /// View access key information.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(%account_id, %public_key)))]
    pub async fn view_access_key(
        &self,
        account_id: &AccountId,
        public_key: &PublicKey,
        block: BlockReference,
    ) -> Result<AccessKeyView, RpcError> {
        let mut params = serde_json::json!({
            "account_id": account_id.to_string(),
            "public_key": public_key.to_string(),
        });
        self.merge_block_reference(&mut params, &block);
        self.call("EXPERIMENTAL_view_access_key", params)
            .await
            .map_err(|e| match e {
                // The EXPERIMENTAL endpoint's UNKNOWN_ACCESS_KEY error omits
                // the account_id from its info payload. Patch it in from the
                // request params so callers get a complete error.
                RpcError::AccessKeyNotFound {
                    public_key,
                    block_height,
                    block_hash,
                    ..
                } => RpcError::AccessKeyNotFound {
                    account_id: account_id.clone(),
                    public_key,
                    block_height,
                    block_hash,
                },
                other => other,
            })
    }

    /// View all access keys for an account.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(%account_id)))]
    pub async fn view_access_key_list(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<AccessKeyListView, RpcError> {
        let mut params = serde_json::json!({
            "account_id": account_id.to_string(),
        });
        self.merge_block_reference(&mut params, &block);
        self.call("EXPERIMENTAL_view_access_key_list", params).await
    }

    /// View the parallel nonces assigned to a gas key.
    ///
    /// This uses the stabilized `query` RPC shape with
    /// `request_type: "view_gas_key_nonces"`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(%account_id, %public_key)))]
    pub async fn view_gas_key_nonces(
        &self,
        account_id: &AccountId,
        public_key: &PublicKey,
        block: BlockReference,
    ) -> Result<GasKeyNoncesView, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_gas_key_nonces",
            "account_id": account_id.to_string(),
            "public_key": public_key.to_string(),
        });
        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await.map_err(|e| match e {
            // UNKNOWN_GAS_KEY's payload omits the account; patch it in from
            // the request params so callers get a complete error.
            RpcError::GasKeyNotFound {
                public_key,
                block_height,
                block_hash,
                ..
            } => RpcError::GasKeyNotFound {
                account_id: account_id.clone(),
                public_key,
                block_height,
                block_hash,
            },
            other => other,
        })
    }

    /// Call a view function on a contract.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, args, block), fields(contract_id = %account_id, method = method_name)))]
    pub async fn view_function(
        &self,
        account_id: &AccountId,
        method_name: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, RpcError> {
        let mut params = serde_json::json!({
            "account_id": account_id.to_string(),
            "method_name": method_name,
            "args_base64": STANDARD.encode(args),
        });
        self.merge_block_reference(&mut params, &block);

        // EXPERIMENTAL_call_function returns errors through the JSON-RPC
        // error envelope, so `parse_rpc_error` handles them. Patch in
        // the caller-known contract_id and method_name when they are
        // missing from the error info (the experimental endpoint omits them).
        let response: CallFunctionResponse = self
            .call("EXPERIMENTAL_call_function", params)
            .await
            .map_err(|e| match e {
                RpcError::ContractExecution {
                    message,
                    block_height,
                    block_hash,
                    ..
                } => RpcError::ContractExecution {
                    contract_id: account_id.clone(),
                    method_name: Some(method_name.to_string()),
                    message,
                    block_height,
                    block_hash,
                },
                RpcError::MethodNotFound {
                    block_height,
                    block_hash,
                    ..
                } => RpcError::MethodNotFound {
                    contract_id: account_id.clone(),
                    method_name: method_name.to_string(),
                    block_height,
                    block_hash,
                },
                other => other,
            })?;

        Ok(ViewFunctionResult {
            result: response.result,
            logs: response.logs,
            block_height: response.block_height,
            block_hash: response.block_hash,
        })
    }

    /// View the WASM code deployed on an account.
    ///
    /// Uses the stabilized `query` RPC shape with `request_type: "view_code"`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(%account_id)))]
    pub async fn view_code(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<ContractCodeView, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_code",
            "account_id": account_id.to_string(),
        });
        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await
    }

    /// View a global contract's WASM code.
    ///
    /// Picks the request type from the identifier kind:
    /// `view_global_contract_code` for code-hash identifiers,
    /// `view_global_contract_code_by_account_id` for publisher accounts.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block), fields(?id)))]
    pub async fn view_global_contract_code(
        &self,
        id: &GlobalContractId,
        block: BlockReference,
    ) -> Result<ContractCodeView, RpcError> {
        let mut params = match id {
            GlobalContractId::CodeHash(hash) => serde_json::json!({
                "request_type": "view_global_contract_code",
                "code_hash": CryptoHash::from_bytes(*hash).to_string(),
            }),
            GlobalContractId::AccountId(account_id) => serde_json::json!({
                "request_type": "view_global_contract_code_by_account_id",
                "account_id": account_id.to_string(),
            }),
        };
        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await.map_err(|e| match e {
            // Replace the error's best-effort identifier with the one the
            // caller asked for, so it stays accurate whatever the node's
            // serialization shape.
            RpcError::GlobalContractNotFound {
                block_height,
                block_hash,
                ..
            } => RpcError::GlobalContractNotFound {
                identifier: match id {
                    GlobalContractId::CodeHash(hash) => {
                        GlobalContractIdentifierView::CodeHash(CryptoHash::from_bytes(*hash))
                    }
                    GlobalContractId::AccountId(account_id) => {
                        GlobalContractIdentifierView::AccountId(account_id.clone())
                    }
                },
                block_height,
                block_hash,
            },
            other => other,
        })
    }

    /// View a single page of a contract's state (raw key/value trie entries).
    ///
    /// Only entries whose key starts with `prefix` are returned (pass an empty
    /// slice for all keys). `after_key` is the continuation cursor from a
    /// previous page's [`ViewStateResult::last_key`], and `limit` caps the
    /// number of entries returned. When the result's `last_key` is `Some`, more
    /// entries remain — call again with `after_key = last_key`.
    ///
    /// Prefer [`RpcClient::view_state_all`] to collect every entry without
    /// managing the cursor yourself.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, prefix, after_key, block), fields(%account_id)))]
    pub async fn view_state(
        &self,
        account_id: &AccountId,
        prefix: &[u8],
        after_key: Option<&[u8]>,
        limit: Option<u32>,
        block: BlockReference,
    ) -> Result<ViewStateResult, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_state",
            "account_id": account_id.to_string(),
            "prefix_base64": STANDARD.encode(prefix),
        });
        if let Some(obj) = params.as_object_mut() {
            if let Some(after) = after_key {
                obj.insert(
                    "after_key_base64".to_string(),
                    STANDARD.encode(after).into(),
                );
            }
            // nearcore requires a non-zero limit; omit it otherwise.
            if let Some(limit) = limit.filter(|&l| l > 0) {
                obj.insert("limit".to_string(), limit.into());
            }
        }
        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await
    }

    /// Read a contract's entire state, transparently following pagination.
    ///
    /// Repeatedly calls [`RpcClient::view_state`] with `page_size` per request,
    /// following the `last_key` cursor until the node reports no more entries,
    /// and returns all matching [`StateItem`]s. Pass an empty `prefix` for the
    /// whole state. `page_size` of `0` means the default page size,
    /// [`RpcClient::DEFAULT_VIEW_STATE_PAGE_SIZE`].
    ///
    /// Every request carries a positive `limit`, so the node treats it as
    /// paginated and does not reject large states with
    /// [`RpcError::ContractStateTooLarge`] — that gate only applies to
    /// unpaginated single-shot queries. Use [`RpcClient::view_state`] with
    /// `limit: None` if you want that one-shot behavior.
    ///
    /// All pages are read against the same `block` so the result is a
    /// consistent snapshot.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, prefix, block), fields(%account_id)))]
    pub async fn view_state_all(
        &self,
        account_id: &AccountId,
        prefix: &[u8],
        page_size: u32,
        block: BlockReference,
    ) -> Result<Vec<StateItem>, RpcError> {
        // Pin a fixed block for the whole scan so every page reads a consistent
        // snapshot. A moving finality reference (`Final`/`Optimistic`/
        // `NearFinal`) would re-resolve to a possibly different block on each
        // page, which can drop or duplicate entries across the cursor; resolve
        // it to a concrete block hash once up front. Already-fixed references
        // (`Height`/`Hash`/`SyncCheckpoint`) are used as-is.
        let fixed_block = match block {
            BlockReference::Finality(_) => {
                let header_hash = self.block(block).await?.header.hash;
                BlockReference::at_hash(header_hash)
            }
            already_fixed => already_fixed,
        };

        // Always send a positive limit: an omitted `limit` on the first page
        // would make the node treat that request as unpaginated and apply
        // its `TOO_LARGE_CONTRACT_STATE` size gate.
        let limit = if page_size > 0 {
            page_size
        } else {
            Self::DEFAULT_VIEW_STATE_PAGE_SIZE
        };
        let mut all = Vec::new();
        let mut after_key: Option<Vec<u8>> = None;
        loop {
            let page = self
                .view_state(
                    account_id,
                    prefix,
                    after_key.as_deref(),
                    Some(limit),
                    fixed_block,
                )
                .await?;
            all.extend(page.values);
            match page.last_key {
                Some(cursor) => after_key = Some(cursor),
                None => break,
            }
        }
        Ok(all)
    }

    /// Get block information.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block)))]
    pub async fn block(&self, block: BlockReference) -> Result<BlockView, RpcError> {
        let params = block.to_rpc_params();
        self.call("block", params).await
    }

    /// Get all state changes that occurred in a block.
    ///
    /// Uses the stabilized `block_effects` method (protocol 2.13), the new name
    /// for `EXPERIMENTAL_changes_in_block`.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, block)))]
    pub async fn block_effects(&self, block: BlockReference) -> Result<BlockEffects, RpcError> {
        let params = block.to_rpc_params();
        self.call("block_effects", params).await
    }

    /// Get the network's genesis configuration as raw JSON.
    ///
    /// Uses the stabilized `genesis_config` method (protocol 2.13), the new name
    /// for `EXPERIMENTAL_genesis_config`. The genesis config is a large,
    /// network-specific document, so it is returned as untyped JSON.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub async fn genesis_config(&self) -> Result<serde_json::Value, RpcError> {
        // Send an empty params array (not `null`) — this is what the other
        // no-arg methods here use, and some JSON-RPC servers require `params` to
        // be an array/object.
        self.call("genesis_config", serde_json::json!([])).await
    }

    /// Get the upcoming maintenance windows for a validator account.
    ///
    /// Each window is a half-open block-height range during which the validator
    /// has no block/chunk production duties. Uses the stabilized
    /// `maintenance_windows` method (protocol 2.13).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(%account_id)))]
    pub async fn maintenance_windows(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MaintenanceWindow>, RpcError> {
        let params = serde_json::json!({ "account_id": account_id.to_string() });
        self.call("maintenance_windows", params).await
    }

    /// Get node status.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub async fn status(&self) -> Result<StatusResponse, RpcError> {
        self.call("status", serde_json::json!([])).await
    }

    /// Get current gas price.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub async fn gas_price(&self, block_hash: Option<&CryptoHash>) -> Result<GasPrice, RpcError> {
        let params = match block_hash {
            Some(hash) => serde_json::json!([hash.to_string()]),
            None => serde_json::json!([serde_json::Value::Null]),
        };
        self.call("gas_price", params).await
    }

    /// Get validator information for an epoch.
    ///
    /// Pass `None` for the latest epoch, or a block height/hash to query a
    /// specific epoch. Finality and sync-checkpoint variants are treated as
    /// latest (the `validators` RPC accepts only `block_id` or `null`).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub async fn validators(
        &self,
        block: Option<BlockReference>,
    ) -> Result<EpochValidatorInfo, RpcError> {
        let params = serde_json::json!([block_id_or_null(block.as_ref())]);
        self.call("validators", params).await
    }

    /// Send a signed transaction.
    ///
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self, signed_tx), fields(
        tx_hash = tracing::field::Empty,
        sender = %signed_tx.transaction.signer_id,
        receiver = %signed_tx.transaction.receiver_id,
        ?wait_until,
    )))]
    pub async fn send_tx(
        &self,
        signed_tx: &SignedTransaction,
        wait_until: TxExecutionStatus,
    ) -> Result<RawTransactionResponse, RpcError> {
        let tx_hash = signed_tx.get_hash();
        trace::Span::current().record("tx_hash", trace::field::display(&tx_hash));
        let params = serde_json::json!({
            "signed_tx_base64": signed_tx.to_base64(),
            "wait_until": wait_until.as_str(),
        });
        let mut response: RawTransactionResponse = self.call("send_tx", params).await?;
        response.transaction_hash = tx_hash;
        Ok(response)
    }

    /// Get transaction status with full receipt details.
    ///
    /// Uses `EXPERIMENTAL_tx_status` which returns complete receipt information.
    /// When the transaction has been executed, the outcome's `receipts` field
    /// will be populated (unlike `send_tx` which leaves it empty).
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(%tx_hash, sender = %sender_id, ?wait_until)))]
    pub async fn tx_status(
        &self,
        tx_hash: &CryptoHash,
        sender_id: &AccountId,
        wait_until: TxExecutionStatus,
    ) -> Result<RawTransactionResponse, RpcError> {
        let params = serde_json::json!({
            "tx_hash": tx_hash.to_string(),
            "sender_account_id": sender_id.to_string(),
            "wait_until": wait_until.as_str(),
        });
        let mut response: RawTransactionResponse =
            self.call("EXPERIMENTAL_tx_status", params).await?;
        response.transaction_hash = response
            .outcome
            .as_ref()
            .map(|o| *o.transaction_hash())
            .unwrap_or(*tx_hash);
        Ok(response)
    }

    /// Look up the transaction that produced a receipt.
    ///
    /// Uses `EXPERIMENTAL_receipt_to_tx`, available on nodes running
    /// nearcore 2.12 or later. Returns [`RpcError::UnknownReceipt`] if the
    /// node does not know the receipt.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self), fields(%receipt_id)))]
    pub async fn receipt_to_tx(
        &self,
        receipt_id: &CryptoHash,
    ) -> Result<ReceiptToTxResponse, RpcError> {
        let params = serde_json::json!({ "receipt_id": receipt_id.to_string() });
        self.call("EXPERIMENTAL_receipt_to_tx", params).await
    }

    /// Merge block reference parameters into a JSON object.
    fn merge_block_reference(&self, params: &mut serde_json::Value, block: &BlockReference) {
        if let serde_json::Value::Object(block_params) = block.to_rpc_params()
            && let serde_json::Value::Object(map) = params
        {
            map.extend(block_params);
        }
    }

    // ========================================================================
    // Sandbox-only methods
    // ========================================================================

    /// Patch account state in sandbox.
    ///
    /// This is a sandbox-only method that allows modifying account state directly,
    /// useful for testing scenarios that require specific account configurations
    /// (e.g., setting a high balance for staking tests).
    ///
    /// # Arguments
    ///
    /// * `records` - State records to patch (Account, Data, Contract, AccessKey, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Set account balance to 1M NEAR
    /// rpc.sandbox_patch_state(serde_json::json!([
    ///     {
    ///         "Account": {
    ///             "account_id": "alice.sandbox",
    ///             "account": {
    ///                 "amount": "1000000000000000000000000000000",
    ///                 "locked": "0",
    ///                 "code_hash": "11111111111111111111111111111111",
    ///                 "storage_usage": 182
    ///             }
    ///         }
    ///     }
    /// ])).await?;
    /// ```
    /// Fast-forward the sandbox by `delta_height` blocks.
    ///
    /// This is useful for testing time-dependent logic (e.g., lockups, staking
    /// epoch changes) without waiting for real block production.
    ///
    /// **Note:** This can take a while for large deltas — the sandbox node
    /// internally produces all intermediate blocks. The RPC call will block
    /// until fast-forwarding completes (up to 1 hour server-side timeout).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Advance the sandbox by 1000 blocks
    /// rpc.sandbox_fast_forward(1000).await?;
    /// ```
    pub async fn sandbox_fast_forward(&self, delta_height: u64) -> Result<(), RpcError> {
        let params = serde_json::json!({
            "delta_height": delta_height,
        });

        let _: serde_json::Value = self.call("sandbox_fast_forward", params).await?;
        Ok(())
    }

    pub async fn sandbox_patch_state(&self, records: serde_json::Value) -> Result<(), RpcError> {
        let params = serde_json::json!({
            "records": records,
        });

        // The sandbox_patch_state method returns an empty result on success
        let _: serde_json::Value = self.call("sandbox_patch_state", params).await?;

        // NOTE: For some reason, patching account-related items sometimes requires
        // sending the patch twice for it to take effect reliably.
        // See: https://github.com/near/near-workspaces-rs/commit/2b72b9b8491c3140ff2d30b0c45d09b200cb027b
        let _: serde_json::Value = self
            .call(
                "sandbox_patch_state",
                serde_json::json!({
                    "records": records,
                }),
            )
            .await?;

        // Small delay to allow state to propagate - sandbox patch_state has race conditions
        async_sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }
}

impl Clone for RpcClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            transport: self.transport.clone(),
            retry_config: self.retry_config.clone(),
            request_id: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcClient")
            .field("url", &self.url)
            .field("retry_config", &self.retry_config)
            .finish()
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Convert a [`BlockReference`] to a `block_id` value for RPC methods that
/// accept only positional `block_id` or `null` (e.g. `validators`,
/// `EXPERIMENTAL_validators_ordered`).
///
/// Height and hash variants are forwarded as-is; finality and sync-checkpoint
/// variants become `null` (latest).
fn block_id_or_null(block: Option<&BlockReference>) -> serde_json::Value {
    match block {
        Some(BlockReference::Height(h)) => serde_json::json!(*h),
        Some(BlockReference::Hash(h)) => serde_json::json!(h.to_string()),
        _ => serde_json::Value::Null,
    }
}

/// Strip query string, fragment, and userinfo from a URL for safe logging.
///
/// RPC provider URLs may carry API keys as query parameters or path tokens.
/// This returns `scheme://host/path` so credentials don't leak into tracing spans.
// Without `tracing`, only the unit tests reference this.
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
fn sanitize_url(url: &str) -> &str {
    // Strip query and fragment
    let end = url.find('?').or_else(|| url.find('#')).unwrap_or(url.len());
    &url[..end]
}

/// Check if an HTTP status code is retryable.
fn is_retryable_status(status: u16) -> bool {
    // 408 Request Timeout - retryable
    // 429 Too Many Requests - retryable (rate limiting)
    // 503 Service Unavailable - retryable
    // 5xx Server Errors - retryable
    status == 408 || status == 429 || status == 503 || (500..600).contains(&status)
}

/// When a non-2xx response is decoded via `parse_rpc_error`, HTTP status is
/// ground truth for retryability: a 4xx response is a deterministic client-side
/// failure and must not retry, regardless of what the JSON-RPC body claims.
///
/// `parse_rpc_error` returns one of two shapes:
///   1. A typed handler variant (UnknownBlock/Chunk/Epoch/RequestTimeout/
///      NodeNotSynced/…). These have well-understood retry semantics already
///      encoded in `RpcError::is_retryable` and should pass through unchanged.
///   2. The catch-all `RpcError::Rpc { code, .. }` (unrecognized or missing
///      `cause.name`). For code `-32000`, `is_retryable` returns `true`, which
///      is appropriate for 5xx (server-side) but *wrong* for 4xx — retrying a
///      deterministic client error wastes time and hides the original failure.
///
/// For case (2) on a 4xx status, downgrade to `RpcError::Network` with
/// `retryable: false` so unrecognized handler causes on 4xx preserve the
/// pre-decode behavior (matches the HTML-body fallback path). 5xx with
/// unknown cause is left as-is — a server-side issue with an unmapped name is
/// plausibly transient and the existing retry semantics are reasonable.
fn preserve_http_retry_classification(err: RpcError, status: u16, body: &str) -> RpcError {
    match err {
        RpcError::Rpc { .. } if (400..500).contains(&status) => {
            RpcError::network(format!("HTTP {}: {}", status, body), Some(status), false)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex, mpsc};

    use reqwest::header::{HeaderMap, HeaderValue};

    use super::*;
    use crate::client::{BoxFuture, TransportResponse};

    struct StaticResponseTransport {
        body: Vec<u8>,
    }

    impl RpcTransport for StaticResponseTransport {
        fn post_json(
            &self,
            _url: &str,
            _body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let body = self.body.clone();
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    /// Transport that returns the same response every time and counts how
    /// many requests it received, so tests can assert on retry attempts.
    struct CountingTransport {
        status: u16,
        body: Vec<u8>,
        calls: AtomicUsize,
    }

    impl CountingTransport {
        fn new(status: u16, body: impl Into<Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                status,
                body: body.into(),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl RpcTransport for CountingTransport {
        fn post_json(
            &self,
            _url: &str,
            _body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let status = self.status;
            let body = self.body.clone();
            Box::pin(async move { Ok(TransportResponse { status, body }) })
        }
    }

    /// Retry config with `max_retries` retries and negligible backoff, so
    /// retry-loop tests don't sleep for real.
    fn fast_retries(max_retries: u32) -> RetryConfig {
        RetryConfig {
            max_retries,
            initial_delay_ms: 1,
            max_delay_ms: 1,
        }
    }

    /// JSON-RPC error body for a `send_tx` rejected with `InvalidNonce`.
    fn invalid_nonce_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": {
                "name": "HANDLER_ERROR",
                "cause": { "name": "INVALID_TRANSACTION", "info": {} },
                "code": -32000,
                "message": "Server error",
                "data": {
                    "TxExecutionError": {
                        "InvalidTxError": {
                            "InvalidNonce": { "tx_nonce": 6, "ak_nonce": 20 }
                        }
                    }
                },
            },
        }))
        .unwrap()
    }

    fn rpc_with_handler_error(cause_name: &str, info: serde_json::Value) -> RpcClient {
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": {
                "name": "HANDLER_ERROR",
                "cause": {
                    "name": cause_name,
                    "info": info,
                },
                "code": -32000,
                "message": "Server error",
                "data": "view function failed",
            },
        }))
        .unwrap();

        RpcClient::with_transport_and_retry_config(
            "https://example.com",
            Arc::new(StaticResponseTransport { body }),
            RetryConfig {
                max_retries: 0,
                ..RetryConfig::default()
            },
        )
    }

    // ========================================================================
    // RetryConfig tests
    // ========================================================================

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 500);
        assert_eq!(config.max_delay_ms, 5000);
    }

    #[test]
    fn test_retry_config_clone() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_retries, 5);
        assert_eq!(cloned.initial_delay_ms, 100);
        assert_eq!(cloned.max_delay_ms, 1000);
    }

    #[test]
    fn test_retry_config_debug() {
        let config = RetryConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("RetryConfig"));
        assert!(debug.contains("max_retries"));
    }

    #[test]
    fn test_retry_config_none() {
        let config = RetryConfig::none();
        assert_eq!(config.max_retries, 0);
        // Delays are irrelevant with zero retries; keep the defaults so a
        // caller doing `RetryConfig { max_retries: 1, ..RetryConfig::none() }`
        // gets sane backoff.
        assert_eq!(config.initial_delay_ms, 500);
        assert_eq!(config.max_delay_ms, 5000);
    }

    // ========================================================================
    // Retry-loop tests (mock transport, counted attempts)
    // ========================================================================

    #[tokio::test]
    async fn test_call_does_not_retry_invalid_nonce() {
        // The signed payload is fixed at this layer: re-sending it
        // byte-for-byte can't fix an `InvalidNonce`, so it must be terminal
        // here even though it is transient one layer up (re-sign).
        let transport = CountingTransport::new(200, invalid_nonce_body());
        let client = RpcClient::with_transport_and_retry_config(
            "https://example.com",
            transport.clone(),
            fast_retries(3),
        );

        let err = client
            .call::<_, serde_json::Value>(
                "send_tx",
                serde_json::json!({ "signed_tx_base64": "AA==", "wait_until": "NONE" }),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(
                err,
                RpcError::InvalidTx(crate::types::InvalidTxError::InvalidNonce {
                    tx_nonce: 6,
                    ak_nonce: 20
                })
            ),
            "expected InvalidTx(InvalidNonce), got {err:?}"
        );
        assert_eq!(transport.calls(), 1, "InvalidNonce must not be re-sent");
    }

    #[tokio::test]
    async fn test_call_retries_shard_congested() {
        // Congestion can clear while the same signed bytes are re-sent, so
        // this InvalidTx variant keeps the transport-level retry loop.
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": {
                "name": "HANDLER_ERROR",
                "cause": { "name": "INVALID_TRANSACTION", "info": {} },
                "code": -32000,
                "message": "Server error",
                "data": {
                    "TxExecutionError": {
                        "InvalidTxError": {
                            "ShardCongested": { "congestion_level": 1.0, "shard_id": 0 }
                        }
                    }
                },
            },
        }))
        .unwrap();
        let transport = CountingTransport::new(200, body);
        let client = RpcClient::with_transport_and_retry_config(
            "https://example.com",
            transport.clone(),
            fast_retries(2),
        );

        let err = client
            .call::<_, serde_json::Value>(
                "send_tx",
                serde_json::json!({ "signed_tx_base64": "AA==", "wait_until": "NONE" }),
            )
            .await
            .unwrap_err();

        assert!(
            matches!(
                err,
                RpcError::InvalidTx(crate::types::InvalidTxError::ShardCongested { .. })
            ),
            "expected InvalidTx(ShardCongested), got {err:?}"
        );
        assert_eq!(transport.calls(), 3, "1 attempt + 2 retries");
    }

    #[tokio::test]
    async fn test_call_retries_transient_transport_errors() {
        // A retryable transport failure (5xx) is retried `max_retries` times.
        let transport = CountingTransport::new(503, "service unavailable");
        let client = RpcClient::with_transport_and_retry_config(
            "https://example.com",
            transport.clone(),
            fast_retries(2),
        );

        let err = client
            .call::<_, serde_json::Value>("block", serde_json::json!({ "finality": "final" }))
            .await
            .unwrap_err();

        assert!(
            matches!(
                err,
                RpcError::Network {
                    status_code: Some(503),
                    retryable: true,
                    ..
                }
            ),
            "expected retryable Network error, got {err:?}"
        );
        assert_eq!(transport.calls(), 3, "1 attempt + 2 retries");
    }

    #[tokio::test]
    async fn test_retry_config_none_makes_a_single_attempt() {
        // With retries disabled, even a retryable error is returned after one
        // attempt — the caller owns the retry policy.
        let transport = CountingTransport::new(503, "service unavailable");
        let client = RpcClient::with_transport_and_retry_config(
            "https://example.com",
            transport.clone(),
            RetryConfig::none(),
        );

        let err = client
            .call::<_, serde_json::Value>("block", serde_json::json!({ "finality": "final" }))
            .await
            .unwrap_err();

        assert!(err.is_retryable(), "503 is retryable in principle: {err:?}");
        assert_eq!(transport.calls(), 1);
    }

    // ========================================================================
    // RpcClient tests
    // ========================================================================

    #[test]
    fn test_rpc_client_new() {
        let client = RpcClient::new("https://rpc.testnet.near.org");
        assert_eq!(client.url(), "https://rpc.testnet.near.org");
    }

    #[test]
    fn test_rpc_client_with_retry_config() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
        };
        let client = RpcClient::with_retry_config("https://rpc.example.com", config);
        assert_eq!(client.url(), "https://rpc.example.com");
    }

    #[test]
    fn test_rpc_client_clone() {
        let client = RpcClient::new("https://rpc.testnet.near.org");
        let cloned = client.clone();
        assert_eq!(cloned.url(), client.url());
    }

    #[test]
    fn test_rpc_client_debug() {
        let client = RpcClient::new("https://rpc.testnet.near.org");
        let debug = format!("{:?}", client);
        assert!(debug.contains("RpcClient"));
        assert!(debug.contains("rpc.testnet.near.org"));
    }

    #[tokio::test]
    async fn test_view_function_distinguishes_contract_errors() {
        let block_height = 243_803_767;
        let block_hash: CryptoHash = "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
            .parse()
            .unwrap();
        let missing_account: AccountId = "missing.near".parse().unwrap();
        let error = rpc_with_handler_error(
            "UNKNOWN_ACCOUNT",
            serde_json::json!({
                "requested_account_id": missing_account,
                "block_height": block_height,
                "block_hash": block_hash,
            }),
        )
        .view_function(
            &missing_account,
            "nep413_get_message",
            &[],
            BlockReference::final_(),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            RpcError::AccountNotFound {
                account_id,
                block_height: Some(actual_block_height),
                block_hash: Some(actual_block_hash),
            } if account_id == missing_account
                && actual_block_height == block_height
                && actual_block_hash == block_hash
        ));

        let account_without_code: AccountId = "no-code.near".parse().unwrap();
        let error = rpc_with_handler_error(
            "CONTRACT_EXECUTION_ERROR",
            serde_json::json!({
                "vm_error": {
                    "CompilationError": {
                        "CodeDoesNotExist": {
                            "account_id": account_without_code,
                        },
                    },
                },
                "block_height": block_height,
                "block_hash": block_hash,
            }),
        )
        .view_function(
            &account_without_code,
            "nep413_get_message",
            &[],
            BlockReference::final_(),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            RpcError::ContractNotDeployed {
                account_id,
                block_height: Some(actual_block_height),
                block_hash: Some(actual_block_hash),
            } if account_id == account_without_code
                && actual_block_height == block_height
                && actual_block_hash == block_hash
        ));

        let contract_id: AccountId = "contract.near".parse().unwrap();
        let missing_method = "nep413_get_message";
        let error = rpc_with_handler_error(
            "CONTRACT_EXECUTION_ERROR",
            serde_json::json!({
                "vm_error": {
                    "MethodResolveError": "MethodNotFound",
                },
                "block_height": block_height,
                "block_hash": block_hash,
            }),
        )
        .view_function(&contract_id, missing_method, &[], BlockReference::final_())
        .await
        .unwrap_err();
        match error {
            RpcError::MethodNotFound {
                contract_id: actual_contract_id,
                method_name,
                block_height: Some(actual_block_height),
                block_hash: Some(actual_block_hash),
            } => {
                assert_eq!(actual_contract_id, contract_id);
                assert_eq!(method_name, missing_method);
                assert_eq!(actual_block_height, block_height);
                assert_eq!(actual_block_hash, block_hash);
            }
            other => panic!("expected MethodNotFound, got {other:?}"),
        }

        let error = rpc_with_handler_error(
            "CONTRACT_EXECUTION_ERROR",
            serde_json::json!({
                "vm_error": {
                    "ExecutionError": "Smart contract panicked: invalid payload",
                },
                "block_height": block_height,
                "block_hash": block_hash,
            }),
        )
        .view_function(&contract_id, "verify_nep413", &[], BlockReference::final_())
        .await
        .unwrap_err();
        match error {
            RpcError::ContractPanic {
                message,
                block_height: Some(actual_block_height),
                block_hash: Some(actual_block_hash),
            } => {
                assert_eq!(message, "invalid payload");
                assert_eq!(actual_block_height, block_height);
                assert_eq!(actual_block_hash, block_hash);
            }
            other => panic!("expected ContractPanic, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_near_builder_uses_custom_http_client() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            request_tx
                .send(String::from_utf8(request).unwrap())
                .unwrap();

            let body = r#"{"jsonrpc":"2.0","id":0,"result":{"ok":true}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body,
            )
            .unwrap();
        });

        let secret = "test-provider-secret";
        let mut headers = HeaderMap::new();
        let mut api_key = HeaderValue::from_static(secret);
        api_key.set_sensitive(true);
        headers.insert("x-api-key", api_key);
        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();
        let near = crate::Near::custom(format!("http://{address}"), "test")
            .http_client(http_client)
            .build();
        let debug = format!("{:?}", near.rpc());
        assert!(!debug.contains(secret));
        assert!(!debug.contains("x-api-key"));
        let response: serde_json::Value = near.rpc().call("status", ()).await.unwrap();
        assert_eq!(response, serde_json::json!({ "ok": true }));

        let request = request_rx.recv().unwrap().to_ascii_lowercase();
        assert!(request.contains(&format!("x-api-key: {secret}")));
        server.join().unwrap();
    }

    // ========================================================================
    // view_state_all pagination
    // ========================================================================

    /// Transport that replays queued JSON-RPC results in order and records the
    /// `params` of every request it was sent.
    struct RecordingTransport {
        responses: Mutex<VecDeque<Vec<u8>>>,
        params: Mutex<Vec<serde_json::Value>>,
    }

    impl RecordingTransport {
        fn new(results: impl IntoIterator<Item = serde_json::Value>) -> Arc<Self> {
            let responses = results
                .into_iter()
                .map(|result| {
                    serde_json::to_vec(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 0,
                        "result": result,
                    }))
                    .unwrap()
                })
                .collect();
            Arc::new(Self {
                responses: Mutex::new(responses),
                params: Mutex::new(Vec::new()),
            })
        }

        fn client(self: &Arc<Self>) -> RpcClient {
            RpcClient::with_transport_and_retry_config(
                "https://example.com",
                self.clone(),
                RetryConfig {
                    max_retries: 0,
                    ..RetryConfig::default()
                },
            )
        }

        fn params(&self) -> Vec<serde_json::Value> {
            self.params.lock().unwrap().clone()
        }
    }

    impl RpcTransport for RecordingTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            self.params.lock().unwrap().push(request["params"].clone());
            let body = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("more RPC requests than queued responses");
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    /// One `view_state` page holding `keys` (all with value `b"v"`), with an
    /// optional `last_key` continuation cursor.
    fn view_state_page(keys: &[&[u8]], last_key: Option<&[u8]>) -> serde_json::Value {
        let values: Vec<_> = keys
            .iter()
            .map(|key| {
                serde_json::json!({ "key": STANDARD.encode(key), "value": STANDARD.encode(b"v") })
            })
            .collect();
        let mut page = serde_json::json!({
            "values": values,
            "block_height": 9u64,
            "block_hash": "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj",
        });
        if let Some(cursor) = last_key {
            page["last_key"] = STANDARD.encode(cursor).into();
        }
        page
    }

    #[tokio::test]
    async fn test_view_state_all_page_size_zero_still_sends_default_limit() {
        // Regression for #294: an omitted `limit` makes nearcore treat the first
        // request as unpaginated and apply the TOO_LARGE_CONTRACT_STATE gate,
        // so `page_size == 0` must still put a positive `limit` on the wire.
        let transport = RecordingTransport::new([
            view_state_page(&[b"sea", b"seb"], Some(b"seb")),
            view_state_page(&[b"sec"], None),
        ]);
        let account: AccountId = "poolv1.near".parse().unwrap();

        let all = transport
            .client()
            .view_state_all(&account, b"se", 0, BlockReference::at_height(1))
            .await
            .unwrap();
        let keys: Vec<&[u8]> = all.iter().map(|item| item.key.as_slice()).collect();
        assert_eq!(keys, [b"sea", b"seb", b"sec"]);

        let params = transport.params();
        assert_eq!(params.len(), 2, "one request per page");
        assert_eq!(params[0]["limit"], RpcClient::DEFAULT_VIEW_STATE_PAGE_SIZE);
        assert_eq!(params[0]["limit"], 10_000);
        assert_eq!(params[0]["prefix_base64"], STANDARD.encode(b"se"));
        assert!(
            params[0].get("after_key_base64").is_none(),
            "first page has no cursor"
        );
        // The second page continues from `last_key` and stays paginated.
        assert_eq!(params[1]["limit"], RpcClient::DEFAULT_VIEW_STATE_PAGE_SIZE);
        assert_eq!(params[1]["after_key_base64"], STANDARD.encode(b"seb"));
    }

    #[tokio::test]
    async fn test_view_state_all_forwards_explicit_page_size() {
        let transport = RecordingTransport::new([view_state_page(&[b"k"], None)]);
        let account: AccountId = "app.near".parse().unwrap();

        let all = transport
            .client()
            .view_state_all(&account, b"", 25, BlockReference::at_height(1))
            .await
            .unwrap();
        assert_eq!(all.len(), 1);

        let params = transport.params();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0]["limit"], 25);
    }

    // ========================================================================
    // sanitize_url tests
    // ========================================================================

    #[test]
    fn test_sanitize_url_plain() {
        assert_eq!(
            sanitize_url("https://rpc.mainnet.near.org"),
            "https://rpc.mainnet.near.org"
        );
    }

    #[test]
    fn test_sanitize_url_strips_query() {
        assert_eq!(
            sanitize_url("https://rpc.provider.com/v1?api_key=secret123"),
            "https://rpc.provider.com/v1"
        );
    }

    #[test]
    fn test_sanitize_url_strips_fragment() {
        assert_eq!(
            sanitize_url("https://rpc.provider.com/v1#section"),
            "https://rpc.provider.com/v1"
        );
    }

    #[test]
    fn test_sanitize_url_strips_query_and_fragment() {
        assert_eq!(
            sanitize_url("https://rpc.provider.com/v1?key=val#frag"),
            "https://rpc.provider.com/v1"
        );
    }

    // ========================================================================
    // is_retryable_status tests
    // ========================================================================

    #[test]
    fn test_is_retryable_status() {
        // Retryable statuses
        assert!(is_retryable_status(408)); // Request Timeout
        assert!(is_retryable_status(429)); // Too Many Requests
        assert!(is_retryable_status(500)); // Internal Server Error
        assert!(is_retryable_status(502)); // Bad Gateway
        assert!(is_retryable_status(503)); // Service Unavailable
        assert!(is_retryable_status(504)); // Gateway Timeout
        assert!(is_retryable_status(599)); // Edge of 5xx range

        // Non-retryable statuses
        assert!(!is_retryable_status(200)); // OK
        assert!(!is_retryable_status(201)); // Created
        assert!(!is_retryable_status(400)); // Bad Request
        assert!(!is_retryable_status(401)); // Unauthorized
        assert!(!is_retryable_status(403)); // Forbidden
        assert!(!is_retryable_status(404)); // Not Found
        assert!(!is_retryable_status(422)); // Unprocessable Entity
    }

    // ========================================================================
    // InvalidTxError parsing tests
    // ========================================================================

    #[test]
    fn test_invalid_transaction_parses_invalid_nonce() {
        use crate::types::InvalidTxError;
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
            RpcError::InvalidTx(InvalidTxError::InvalidNonce { tx_nonce, ak_nonce }) => {
                assert_eq!(tx_nonce, 5);
                assert_eq!(ak_nonce, 10);
            }
            other => panic!("Expected InvalidTx(InvalidNonce), got: {other:?}"),
        }
    }

    #[test]
    fn test_invalid_transaction_parses_top_level_invalid_tx() {
        use crate::types::InvalidTxError;
        // Some RPC versions put InvalidTxError at the top level
        let data = serde_json::json!({
            "InvalidTxError": {
                "NotEnoughBalance": {
                    "signer_id": "alice.near",
                    "balance": "1000000000000000000000000",
                    "cost": "9000000000000000000000000"
                }
            }
        });
        let err = RpcError::invalid_transaction("insufficient balance", Some(data));
        assert!(
            matches!(
                err,
                RpcError::InvalidTx(InvalidTxError::NotEnoughBalance { .. })
            ),
            "Expected InvalidTx(NotEnoughBalance), got: {err:?}"
        );
    }

    #[test]
    fn test_invalid_transaction_falls_back_on_unparseable() {
        // When data doesn't contain a parseable InvalidTxError, falls back
        let data = serde_json::json!({ "SomeOtherError": {} });
        let err = RpcError::invalid_transaction("some error", Some(data));
        assert!(matches!(err, RpcError::InvalidTransaction { .. }));
    }

    // ========================================================================
    // NetworkConfig tests
    // ========================================================================

    #[test]
    fn test_mainnet_config() {
        assert!(MAINNET.rpc_url.contains("fastnear"));
        assert_eq!(MAINNET.network_id, "mainnet");
    }

    #[test]
    fn test_testnet_config() {
        assert!(TESTNET.rpc_url.contains("fastnear") || TESTNET.rpc_url.contains("test"));
        assert_eq!(TESTNET.network_id, "testnet");
    }

    // ========================================================================
    // parse_rpc_error tests (via RpcClient)
    // ========================================================================

    #[test]
    fn test_parse_rpc_error_unknown_account() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "UNKNOWN_ACCOUNT".to_string(),
                info: Some(serde_json::json!({
                    "requested_account_id": "nonexistent.near",
                    "block_height": 243803761,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(
            result,
            RpcError::AccountNotFound {
                block_height: Some(243_803_761),
                block_hash: Some(CryptoHash::ZERO),
                ..
            }
        ));
    }

    #[test]
    fn test_parse_rpc_error_unknown_access_key_legacy() {
        // Legacy `query` endpoint includes requested_account_id in info
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "UNKNOWN_ACCESS_KEY".to_string(),
                info: Some(serde_json::json!({
                    "requested_account_id": "alice.near",
                    "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::AccessKeyNotFound {
                account_id,
                public_key,
                block_height,
                block_hash,
            } => {
                assert_eq!(account_id.as_str(), "alice.near");
                assert!(public_key.to_string().contains("ed25519:"));
                assert_eq!(block_height, None);
                assert_eq!(block_hash, None);
            }
            _ => panic!("Expected AccessKeyNotFound error, got {:?}", result),
        }
    }

    #[test]
    fn test_parse_rpc_error_unknown_access_key_experimental() {
        // EXPERIMENTAL_view_access_key omits requested_account_id from info
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: Some(serde_json::Value::String(
                "Access key for public key ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp does not exist while viewing".to_string()
            )),
            cause: Some(ErrorCause {
                name: "UNKNOWN_ACCESS_KEY".to_string(),
                info: Some(serde_json::json!({
                    "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                    "block_height": 243789592,
                    "block_hash": "EC5A7qc6rixfN8T4T9Gkt78H5pAsvdcjAos8Z7kFLJgi"
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::AccessKeyNotFound {
                account_id,
                public_key,
                block_height,
                block_hash,
            } => {
                // account_id falls back to "unknown" — caller enriches it
                assert_eq!(account_id.as_str(), "unknown");
                assert!(public_key.to_string().contains("ed25519:"));
                assert_eq!(block_height, Some(243_789_592));
                assert_eq!(
                    block_hash.map(|h| h.to_string()).as_deref(),
                    Some("EC5A7qc6rixfN8T4T9Gkt78H5pAsvdcjAos8Z7kFLJgi")
                );
            }
            _ => panic!("Expected AccessKeyNotFound error, got {:?}", result),
        }
    }

    #[test]
    fn test_parse_rpc_error_unknown_gas_key() {
        // nearcore's `RpcQueryError::UnknownGasKey { public_key, block_height,
        // block_hash }` as serialized by the `query` endpoint for
        // `view_gas_key_nonces`; the account is not on the wire.
        let client = RpcClient::new("https://example.com");
        let error: JsonRpcError = serde_json::from_value(serde_json::json!({
            "code": -32000,
            "message": "Server error",
            "data": "Gas key for public key ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp does not exist while viewing",
            "name": "HANDLER_ERROR",
            "cause": {
                "name": "UNKNOWN_GAS_KEY",
                "info": {
                    "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                    "block_height": 243789592,
                    "block_hash": "EC5A7qc6rixfN8T4T9Gkt78H5pAsvdcjAos8Z7kFLJgi"
                }
            }
        }))
        .unwrap();
        match client.parse_rpc_error(&error) {
            RpcError::GasKeyNotFound {
                account_id,
                public_key,
                block_height,
                block_hash,
            } => {
                assert_eq!(account_id.as_str(), "unknown");
                assert_eq!(
                    public_key.to_string(),
                    "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                );
                assert_eq!(block_height, Some(243_789_592));
                assert_eq!(
                    block_hash.map(|h| h.to_string()).as_deref(),
                    Some("EC5A7qc6rixfN8T4T9Gkt78H5pAsvdcjAos8Z7kFLJgi")
                );
            }
            other => panic!("expected GasKeyNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_view_gas_key_nonces_patches_account_into_gas_key_not_found() {
        let account_id: AccountId = "alice.near".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let error = rpc_with_handler_error(
            "UNKNOWN_GAS_KEY",
            serde_json::json!({
                "public_key": public_key,
                "block_height": 100,
                "block_hash": "11111111111111111111111111111111",
            }),
        )
        .view_gas_key_nonces(&account_id, &public_key, BlockReference::final_())
        .await
        .unwrap_err();
        assert!(
            matches!(
                &error,
                RpcError::GasKeyNotFound {
                    account_id: actual_account,
                    public_key: actual_key,
                    block_height: Some(100),
                    block_hash: Some(CryptoHash::ZERO),
                } if *actual_account == account_id && *actual_key == public_key
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn test_parse_rpc_error_invalid_account() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "INVALID_ACCOUNT".to_string(),
                info: Some(serde_json::json!({
                    "requested_account_id": "invalid@account",
                    "block_height": 243803761,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(
            matches!(
                &result,
                RpcError::InvalidAccount {
                    account_id,
                    block_height: Some(243_803_761),
                    block_hash: Some(CryptoHash::ZERO),
                } if account_id == "invalid@account"
            ),
            "got {result:?}"
        );
    }

    #[test]
    fn test_parse_rpc_error_unknown_block() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Block not found".to_string(),
            data: Some(serde_json::json!("12345")),
            cause: Some(ErrorCause {
                name: "UNKNOWN_BLOCK".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::UnknownBlock(_)));
    }

    #[test]
    fn test_parse_rpc_error_unknown_chunk() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Chunk not found".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "UNKNOWN_CHUNK".to_string(),
                info: Some(serde_json::json!({
                    "chunk_hash": "abc123"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::UnknownChunk(_)));
    }

    #[test]
    fn test_parse_rpc_error_unknown_epoch() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Epoch not found".to_string(),
            data: Some(serde_json::json!("epoch123")),
            cause: Some(ErrorCause {
                name: "UNKNOWN_EPOCH".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::UnknownEpoch(_)));
    }

    #[test]
    fn test_parse_rpc_error_unknown_receipt() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Receipt not found".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "UNKNOWN_RECEIPT".to_string(),
                info: Some(serde_json::json!({
                    "receipt_id": "receipt123"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::UnknownReceipt(_)));
    }

    // ========================================================================
    // EXPERIMENTAL_receipt_to_tx tests
    //
    // There is no HTTP mock harness in this crate, so the success path is
    // exercised by decoding a real-shape `result` envelope into the
    // user-facing type (the same step `try_call` performs), and the error
    // path is exercised through `parse_rpc_error` on the real error envelope.
    // ========================================================================

    #[test]
    fn test_receipt_to_tx_decodes_success_body() {
        // Real-shape success body nearcore returns for EXPERIMENTAL_receipt_to_tx.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "transaction_hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U",
                "sender_account_id": "alice.near"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        assert!(parsed.error.is_none(), "no error envelope expected");
        let result = parsed.result.expect("result present");
        let response: ReceiptToTxResponse =
            serde_json::from_value(result).expect("decodes into ReceiptToTxResponse");
        assert_eq!(
            response.transaction_hash.to_string(),
            "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"
        );
        assert_eq!(response.sender_account_id.as_str(), "alice.near");
    }

    #[test]
    fn test_receipt_to_tx_maps_unknown_receipt() {
        // Real-shape error body nearcore returns for an unknown receipt.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "cause": {
                    "name": "UNKNOWN_RECEIPT",
                    "info": {
                        "receipt_id": "3GTGoiN3FEoJenSw5ob4YMmFEV2Fbiichj3FDBnM78xK"
                    }
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let result = client.parse_rpc_error(&error);
        assert!(
            matches!(result, RpcError::UnknownReceipt(_)),
            "expected UnknownReceipt, got {:?}",
            result
        );
    }

    #[test]
    fn test_parse_rpc_error_no_contract_code() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "No contract code".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "NO_CONTRACT_CODE".to_string(),
                info: Some(serde_json::json!({
                    "contract_account_id": "no-contract.near",
                    "block_height": 243803762,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(
            result,
            RpcError::ContractNotDeployed {
                block_height: Some(243_803_762),
                block_hash: Some(CryptoHash::ZERO),
                ..
            }
        ));
    }

    fn no_global_contract_code_error(identifier: serde_json::Value) -> JsonRpcError {
        JsonRpcError {
            code: -32000,
            message: "No global contract code".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "NO_GLOBAL_CONTRACT_CODE".to_string(),
                info: Some(serde_json::json!({
                    "identifier": identifier,
                    "block_height": 100,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: None,
        }
    }

    #[test]
    fn test_parse_rpc_error_no_global_contract_code() {
        let client = RpcClient::new("https://example.com");

        // Account-id identifier
        let error =
            no_global_contract_code_error(serde_json::json!({ "account_id": "publisher.near" }));
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::GlobalContractNotFound {
                identifier: GlobalContractIdentifierView::AccountId(id),
                block_height,
                block_hash,
            } => {
                assert_eq!(id.as_str(), "publisher.near");
                assert_eq!(block_height, Some(100));
                assert_eq!(block_hash, Some(CryptoHash::ZERO));
            }
            other => panic!("expected GlobalContractNotFound(AccountId), got {other:?}"),
        }

        // Code-hash identifier
        let error = no_global_contract_code_error(
            serde_json::json!({ "hash": "11111111111111111111111111111111" }),
        );
        let result = client.parse_rpc_error(&error);
        assert!(matches!(
            result,
            RpcError::GlobalContractNotFound {
                identifier: GlobalContractIdentifierView::CodeHash(_),
                ..
            }
        ));
    }

    #[test]
    fn test_parse_rpc_error_no_global_contract_code_pre_2_12_shape() {
        let client = RpcClient::new("https://example.com");

        // Before nearcore#15539 (2.12.0), the identifier serialized with
        // variant names instead of the view field names.
        let error =
            no_global_contract_code_error(serde_json::json!({ "AccountId": "publisher.near" }));
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::GlobalContractNotFound {
                identifier: GlobalContractIdentifierView::AccountId(id),
                ..
            } => {
                assert_eq!(id.as_str(), "publisher.near");
            }
            other => panic!("expected GlobalContractNotFound(AccountId), got {other:?}"),
        }

        let error = no_global_contract_code_error(
            serde_json::json!({ "CodeHash": "11111111111111111111111111111111" }),
        );
        let result = client.parse_rpc_error(&error);
        assert!(matches!(
            result,
            RpcError::GlobalContractNotFound {
                identifier: GlobalContractIdentifierView::CodeHash(_),
                ..
            }
        ));
    }

    #[test]
    fn test_parse_rpc_error_no_global_contract_code_unparseable_identifier() {
        let client = RpcClient::new("https://example.com");

        // An unrecognized identifier shape must not demote the error to the
        // generic Rpc variant — the cause name alone identifies it.
        let error = no_global_contract_code_error(serde_json::json!({ "unexpected": 42 }));
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::GlobalContractNotFound { .. }));

        // Same when the identifier field is missing entirely.
        let error = JsonRpcError {
            code: -32000,
            message: "No global contract code".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "NO_GLOBAL_CONTRACT_CODE".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::GlobalContractNotFound { .. }));
    }

    #[test]
    fn test_parse_rpc_error_too_large_contract_state() {
        let client = RpcClient::new("https://example.com");
        // Real wire payload from rpc.mainnet.near.org: nearcore keys the
        // account as `contract_account_id`, not `account_id`.
        let error: JsonRpcError = serde_json::from_value(serde_json::json!({
            "code": -32000,
            "message": "Server error",
            "data": "State of contract wrap.near is too large to be viewed",
            "name": "HANDLER_ERROR",
            "cause": {
                "name": "TOO_LARGE_CONTRACT_STATE",
                "info": {
                    "block_hash": "E83FeM6Z7HDJ1W4VtZyhRHdpP6YYttJQe6T7N9LQNW2S",
                    "block_height": 211889547,
                    "contract_account_id": "wrap.near"
                }
            }
        }))
        .unwrap();
        match client.parse_rpc_error(&error) {
            RpcError::ContractStateTooLarge {
                account_id,
                block_height,
                block_hash,
            } => {
                assert_eq!(account_id.as_str(), "wrap.near");
                assert_eq!(block_height, Some(211_889_547));
                assert_eq!(
                    block_hash.map(|h| h.to_string()).as_deref(),
                    Some("E83FeM6Z7HDJ1W4VtZyhRHdpP6YYttJQe6T7N9LQNW2S")
                );
            }
            other => panic!("expected ContractStateTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_too_large_contract_state_legacy_account_id_key() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Contract state too large".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "TOO_LARGE_CONTRACT_STATE".to_string(),
                info: Some(serde_json::json!({
                    "account_id": "large-state.near"
                })),
            }),
            name: None,
        };
        match client.parse_rpc_error(&error) {
            RpcError::ContractStateTooLarge {
                account_id,
                block_height: None,
                block_hash: None,
            } => {
                assert_eq!(account_id.as_str(), "large-state.near");
            }
            other => panic!("expected ContractStateTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_too_large_contract_state_without_account_falls_through() {
        let client = RpcClient::new("https://example.com");
        // No recognizable account key: don't invent "unknown", surface the
        // raw handler error instead.
        let error = JsonRpcError {
            code: -32000,
            message: "Contract state too large".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "TOO_LARGE_CONTRACT_STATE".to_string(),
                info: Some(serde_json::json!({
                    "block_height": 211889547
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(
            matches!(result, RpcError::Rpc { code: -32000, .. }),
            "expected generic Rpc error, got {result:?}"
        );
    }

    #[test]
    fn test_parse_rpc_error_unavailable_shard() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Shard unavailable".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "UNAVAILABLE_SHARD".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::ShardUnavailable(_)));
    }

    #[test]
    fn test_parse_rpc_error_not_synced() {
        let client = RpcClient::new("https://example.com");

        // NO_SYNCED_BLOCKS
        let error = JsonRpcError {
            code: -32000,
            message: "No synced blocks".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "NO_SYNCED_BLOCKS".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::NodeNotSynced(_)));

        // NOT_SYNCED_YET
        let error = JsonRpcError {
            code: -32000,
            message: "Not synced yet".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "NOT_SYNCED_YET".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::NodeNotSynced(_)));
    }

    #[test]
    fn test_parse_rpc_error_invalid_shard_id() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Invalid shard ID".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "INVALID_SHARD_ID".to_string(),
                info: Some(serde_json::json!({
                    "shard_id": 99
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::InvalidShardId(_)));
    }

    #[test]
    fn test_parse_rpc_error_invalid_transaction() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Invalid transaction".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "INVALID_TRANSACTION".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::InvalidTransaction { .. }));
    }

    #[test]
    fn test_parse_rpc_error_timeout() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Request timed out".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "TIMEOUT_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "transaction_hash": "tx123"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::RequestTimeout { .. }));
    }

    #[test]
    fn test_parse_rpc_error_parse_error() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32700,
            message: "Parse error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "PARSE_ERROR".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::ParseError(_)));
    }

    #[test]
    fn test_parse_rpc_error_internal_error() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32603,
            message: "Internal error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "INTERNAL_ERROR".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::InternalError(_)));
    }

    #[test]
    fn test_parse_rpc_error_contract_execution_legacy() {
        // Legacy `query` endpoint includes contract_id and method_name in info
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Contract execution failed".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "contract_id": "contract.near",
                    "method_name": "my_method"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::ContractExecution {
                contract_id,
                method_name,
                ..
            } => {
                assert_eq!(contract_id.as_str(), "contract.near");
                assert_eq!(method_name.as_deref(), Some("my_method"));
            }
            _ => panic!("Expected ContractExecution error, got {:?}", result),
        }
    }

    #[test]
    fn test_parse_rpc_error_method_not_found_current_shape() {
        // EXPERIMENTAL_call_function omits contract_id/method_name from info,
        // but includes vm_error and a data string
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: Some(serde_json::json!(
                "Function call returned an error: MethodResolveError(MethodNotFound)"
            )),
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "vm_error": { "MethodResolveError": "MethodNotFound" },
                    "block_height": 243803767,
                    "block_hash": "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::MethodNotFound {
                contract_id,
                method_name,
                block_height,
                block_hash,
            } => {
                // The high-level view_function caller enriches both fields.
                assert_eq!(contract_id.as_str(), "unknown");
                assert_eq!(method_name, "unknown");
                assert_eq!(block_height, Some(243_803_767));
                assert_eq!(
                    block_hash,
                    Some(
                        "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
                            .parse()
                            .unwrap()
                    )
                );
            }
            _ => panic!("Expected MethodNotFound error, got {:?}", result),
        }
    }

    #[test]
    fn test_parse_rpc_error_method_not_found_legacy_error_field() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    // The endpoint's initial structured shape called this
                    // field `error`; current nearcore calls it `vm_error`.
                    "error": { "MethodResolveError": "MethodNotFound" },
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };

        assert!(matches!(
            client.parse_rpc_error(&error),
            RpcError::MethodNotFound { .. }
        ));
    }

    #[test]
    fn test_parse_rpc_error_contract_panic_current_shape() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "vm_error": {
                        "ExecutionError": "Smart contract panicked: invalid signature"
                    },
                    "block_height": 243803768,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic {
                message,
                block_height: Some(243_803_768),
                block_hash: Some(CryptoHash::ZERO),
            } => assert_eq!(message, "invalid signature"),
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_contract_panic_real_testnet_payload() {
        // Verbatim testnet `EXPERIMENTAL_call_function` response for a near-sdk
        // contract that panicked while deserializing its arguments.
        let client = RpcClient::new("https://example.com");
        let panic_msg = "panicked at 'Failed to deserialize input from JSON.: Error(\"missing field `keys`\", line: 1, column: 2)', contract/src/api.rs:54:1";
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: Some(serde_json::json!(format!(
                "Function call returned an error: ExecutionError({:?})",
                format!("Smart contract panicked: {panic_msg}")
            ))),
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "vm_error": {
                        "ExecutionError": format!("Smart contract panicked: {panic_msg}"),
                    },
                    "block_height": 263803636u64,
                    "block_hash": "B7UiEhkg1AeXvUoJUEaf8cqEc8Py7XhZ66vt7jJJUHw5",
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic {
                message,
                block_height,
                block_hash,
            } => {
                assert_eq!(message, panic_msg);
                assert_eq!(block_height, Some(263_803_636));
                assert_eq!(
                    block_hash,
                    Some(
                        "B7UiEhkg1AeXvUoJUEaf8cqEc8Py7XhZ66vt7jJJUHw5"
                            .parse()
                            .unwrap()
                    )
                );
            }
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_contract_panic_message_preserves_source_casing() {
        // The text after the prefix belongs to the contract author, so it is
        // passed through byte for byte rather than re-cased.
        assert_eq!(
            extract_contract_panic_message("Smart contract panicked: Insufficient balance"),
            Some("Insufficient balance".to_string())
        );
        assert_eq!(
            extract_contract_panic_message("Smart contract panicked: ERR_NOT_ENOUGH_DEPOSIT"),
            Some("ERR_NOT_ENOUGH_DEPOSIT".to_string())
        );
    }

    #[test]
    fn test_extract_contract_panic_message_ignores_non_panic_errors() {
        assert_eq!(
            extract_contract_panic_message("memory access violation"),
            None
        );
        assert_eq!(extract_contract_panic_message("Smart contract"), None);
        assert_eq!(
            extract_contract_panic_message("Smart contract panicked unexpectedly"),
            None
        );
    }

    #[test]
    fn test_extract_contract_panic_message_keeps_prefix_without_a_message() {
        // Stripping would leave nothing to show, so the raw string is kept.
        assert_eq!(
            extract_contract_panic_message("Smart contract panicked"),
            Some("Smart contract panicked".to_string())
        );
        assert_eq!(
            extract_contract_panic_message("Smart contract panicked:   "),
            Some("Smart contract panicked:   ".to_string())
        );
    }

    #[test]
    fn test_parse_rpc_error_non_panic_execution_error_stays_generic() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "vm_error": {
                        "ExecutionError": "memory access violation"
                    },
                    "block_height": 243803769,
                    "block_hash": "11111111111111111111111111111111"
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };

        match client.parse_rpc_error(&error) {
            RpcError::ContractExecution {
                message,
                block_height: Some(243_803_769),
                block_hash: Some(CryptoHash::ZERO),
                ..
            } => {
                assert!(message.contains("memory access violation"));
            }
            other => panic!("Expected ContractExecution error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_contract_panic_legacy_guest_panic_shape() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: None,
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "error": {
                        "HostError": {
                            "GuestPanic": {
                                "panic_msg": "legacy panic"
                            }
                        }
                    },
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic { message, .. } => assert_eq!(message, "legacy panic"),
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_code_does_not_exist_experimental() {
        // EXPERIMENTAL_call_function returns CodeDoesNotExist as CONTRACT_EXECUTION_ERROR
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: Some(serde_json::json!(
                "Function call returned an error: CompilationError(CodeDoesNotExist { account_id: AccountId(\"nonexistent.testnet\") })"
            )),
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info: Some(serde_json::json!({
                    "vm_error": {
                        "CompilationError": {
                            "CodeDoesNotExist": {
                                "account_id": "nonexistent.testnet"
                            }
                        }
                    },
                    "block_height": 243803764,
                    "block_hash": "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
                })),
            }),
            name: Some("HANDLER_ERROR".to_string()),
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::ContractNotDeployed {
                account_id,
                block_height,
                block_hash,
            } => {
                assert_eq!(account_id.as_str(), "nonexistent.testnet");
                assert_eq!(block_height, Some(243_803_764));
                assert_eq!(
                    block_hash,
                    Some(
                        "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
                            .parse()
                            .unwrap()
                    )
                );
            }
            _ => panic!("Expected ContractNotDeployed error, got {:?}", result),
        }
    }

    /// Build a `CONTRACT_EXECUTION_ERROR` whose only description of the
    /// failure is the string form: no structured `vm_error` anywhere.
    fn legacy_string_error(data: Option<&str>, info: Option<serde_json::Value>) -> JsonRpcError {
        JsonRpcError {
            code: -32000,
            message: "Server error".to_string(),
            data: data.map(|d| serde_json::json!(d)),
            cause: Some(ErrorCause {
                name: "CONTRACT_EXECUTION_ERROR".to_string(),
                info,
            }),
            name: Some("HANDLER_ERROR".to_string()),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_method_not_found() {
        let client = RpcClient::new("https://example.com");
        let error = legacy_string_error(
            Some("Function call returned an error: MethodResolveError(MethodNotFound)"),
            None,
        );

        match client.parse_rpc_error(&error) {
            RpcError::MethodNotFound {
                contract_id,
                method_name,
                block_height: None,
                block_hash: None,
            } => {
                assert_eq!(contract_id.as_str(), "unknown");
                assert_eq!(method_name, "unknown");
            }
            other => panic!("Expected MethodNotFound error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_method_not_found_with_info_context() {
        // `info` is present (legacy `query` shape with contract/method and
        // block context) but carries no structured `vm_error`.
        let client = RpcClient::new("https://example.com");
        let error = legacy_string_error(
            Some("Function call returned an error: MethodResolveError(MethodNotFound)"),
            Some(serde_json::json!({
                "contract_id": "contract.near",
                "method_name": "my_method",
                "block_height": 243803767,
                "block_hash": "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
            })),
        );

        match client.parse_rpc_error(&error) {
            RpcError::MethodNotFound {
                contract_id,
                method_name,
                block_height,
                block_hash,
            } => {
                assert_eq!(contract_id.as_str(), "contract.near");
                assert_eq!(method_name, "my_method");
                assert_eq!(block_height, Some(243_803_767));
                assert_eq!(
                    block_hash,
                    Some(
                        "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj"
                            .parse()
                            .unwrap()
                    )
                );
            }
            other => panic!("Expected MethodNotFound error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_code_does_not_exist() {
        let client = RpcClient::new("https://example.com");
        let error = legacy_string_error(
            Some(
                "Function call returned an error: CompilationError(CodeDoesNotExist { account_id: AccountId(\"nonexistent.testnet\") })",
            ),
            Some(serde_json::json!({
                "block_height": 243803764,
                "block_hash": "11111111111111111111111111111111"
            })),
        );

        match client.parse_rpc_error(&error) {
            RpcError::ContractNotDeployed {
                account_id,
                block_height: Some(243_803_764),
                block_hash: Some(CryptoHash::ZERO),
            } => assert_eq!(account_id.as_str(), "nonexistent.testnet"),
            other => panic!("Expected ContractNotDeployed error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_guest_panic_in_vm_error_string() {
        // Legacy `query` shape: `vm_error` is the Debug rendering as a string,
        // and there is no `data` at all.
        let client = RpcClient::new("https://example.com");
        let error = legacy_string_error(
            None,
            Some(serde_json::json!({
                "vm_error": "wasm execution failed with error: HostError(GuestPanic { panic_msg: \"assertion failed: \\\"a\\\" != \\\"b\\\"\" })",
            })),
        );

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic { message, .. } => {
                assert_eq!(message, "assertion failed: \"a\" != \"b\"");
            }
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_execution_error_panic() {
        let client = RpcClient::new("https://example.com");
        let panic_msg = "panicked at 'Failed to deserialize input from JSON.: Error(\"missing field `keys`\", line: 1, column: 2)', contract/src/api.rs:54:1";
        let error = legacy_string_error(
            Some(&format!(
                "Function call returned an error: ExecutionError({:?})",
                format!("Smart contract panicked: {panic_msg}")
            )),
            None,
        );

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic { message, .. } => assert_eq!(message, panic_msg),
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rpc_error_legacy_string_unknown_stays_generic() {
        let client = RpcClient::new("https://example.com");
        for data in [
            "Function call returned an error: WasmTrap(Unreachable)",
            // A non-panic ExecutionError shares the container variant with
            // panics but must not be promoted.
            "Function call returned an error: ExecutionError(\"memory access violation\")",
            // A known token embedded in another variant's free text does not
            // make that variant the outer error.
            "Function call returned an error: LinkError { msg: \"MethodResolveError(MethodNotFound)\" }",
        ] {
            match client.parse_rpc_error(&legacy_string_error(Some(data), None)) {
                RpcError::ContractExecution { message, .. } => assert_eq!(message, data),
                other => panic!("Expected ContractExecution error for {data:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_parse_rpc_error_structured_vm_error_wins_over_legacy_string() {
        // When both shapes are present the structured field is authoritative,
        // even if the string disagrees.
        let client = RpcClient::new("https://example.com");
        let error = legacy_string_error(
            Some("Function call returned an error: MethodResolveError(MethodNotFound)"),
            Some(serde_json::json!({
                "vm_error": { "HostError": { "GuestPanic": { "panic_msg": "structured" } } },
            })),
        );

        match client.parse_rpc_error(&error) {
            RpcError::ContractPanic { message, .. } => assert_eq!(message, "structured"),
            other => panic!("Expected ContractPanic error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_debug_str_unescapes_rust_debug_output() {
        let original = "quote \" backslash \\ newline \n tab \t bell \u{7} unicode é 🚀";
        let rendered = format!("{original:?} trailing");
        assert_eq!(parse_debug_str(&rendered).as_deref(), Some(original));
        // Unterminated or non-literal input is rejected rather than guessed.
        assert_eq!(parse_debug_str("\"unterminated"), None);
        assert_eq!(parse_debug_str("not a literal"), None);
    }

    #[test]
    fn test_parse_rpc_error_fallback_account_not_exist() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Error".to_string(),
            data: Some(serde_json::json!(
                "account missing.near does not exist while viewing"
            )),
            cause: None,
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::AccountNotFound { .. }));
    }

    #[test]
    fn test_parse_rpc_error_unknown_cause_fallback_to_generic() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32000,
            message: "Some error".to_string(),
            data: Some(serde_json::json!("some data")),
            cause: Some(ErrorCause {
                name: "UNKNOWN_ERROR_TYPE".to_string(),
                info: None,
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::Rpc { .. }));
    }

    #[test]
    fn test_parse_rpc_error_no_cause_fallback_to_generic() {
        let client = RpcClient::new("https://example.com");
        let error = JsonRpcError {
            code: -32600,
            message: "Invalid request".to_string(),
            data: None,
            cause: None,
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::Rpc { code, message, .. } => {
                assert_eq!(code, -32600);
                assert_eq!(message, "Invalid request");
            }
            _ => panic!("Expected generic Rpc error"),
        }
    }

    // ========================================================================
    // non-2xx envelope decode tests
    //
    // nearcore returns HTTP 422 (UNKNOWN_BLOCK/UNKNOWN_CHUNK) and 408
    // (TIMEOUT_ERROR) with a well-formed JSON-RPC error body. `try_call` now
    // tries to decode that body before falling back to `RpcError::Network`.
    // These tests verify the decode path on synthetic bodies without needing
    // an HTTP mock harness.
    // ========================================================================

    #[test]
    fn test_non_2xx_body_decodes_unknown_block() {
        // Real-shape body nearcore returns with HTTP 422 for UNKNOWN_BLOCK.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "data": "DB Not Found Error: BLOCK HEIGHT: 1",
                "cause": {
                    "name": "UNKNOWN_BLOCK",
                    "info": {}
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let result = client.parse_rpc_error(&error);
        assert!(
            matches!(result, RpcError::UnknownBlock(_)),
            "expected UnknownBlock, got {:?}",
            result
        );
    }

    #[test]
    fn test_non_2xx_body_decodes_unknown_chunk() {
        // Real-shape body nearcore returns with HTTP 422 for UNKNOWN_CHUNK.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "cause": {
                    "name": "UNKNOWN_CHUNK",
                    "info": {
                        "chunk_hash": "3tMcx4v6hUzN7XeQgEr4kQb8R5rGvmM4Py7o4nP1T8bY"
                    }
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let result = client.parse_rpc_error(&error);
        assert!(
            matches!(result, RpcError::UnknownChunk(_)),
            "expected UnknownChunk, got {:?}",
            result
        );
    }

    #[test]
    fn test_non_2xx_body_decodes_timeout() {
        // Real-shape body nearcore returns with HTTP 408 for TIMEOUT_ERROR.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Timeout",
                "cause": {
                    "name": "TIMEOUT_ERROR",
                    "info": {
                        "transaction_hash": "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U"
                    }
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let result = client.parse_rpc_error(&error);
        match result {
            RpcError::RequestTimeout {
                transaction_hash, ..
            } => {
                assert_eq!(
                    transaction_hash.as_deref(),
                    Some("9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U")
                );
            }
            _ => panic!("expected RequestTimeout, got {:?}", result),
        }
    }

    #[test]
    fn test_non_2xx_html_body_falls_back_to_network() {
        // Non-JSON bodies (HTML error pages from proxies, gateways, etc.)
        // must still fall through to `RpcError::Network` with the status code
        // preserved. We verify the decode-attempt fails so try_call's fallback
        // path kicks in.
        let body = "<html><body><h1>422 Unprocessable Entity</h1></body></html>";
        let parsed = serde_json::from_str::<JsonRpcResponse>(body);
        assert!(
            parsed.is_err(),
            "HTML body must fail to parse as JsonRpcResponse so try_call falls back to Network"
        );

        // Confirm the fallback produces the expected shape (same call try_call
        // makes once the decode attempt fails).
        let fallback = RpcError::network(format!("HTTP {}: {}", 422, body), Some(422), false);
        match fallback {
            RpcError::Network {
                status_code,
                retryable,
                ..
            } => {
                assert_eq!(status_code, Some(422));
                assert!(!retryable, "422 should not be retryable");
            }
            _ => panic!("expected Network error"),
        }
    }

    // ========================================================================
    // preserve_http_retry_classification tests
    //
    // Regression coverage for PR #189: 4xx responses with unrecognized
    // handler causes must stay non-retryable. Without the downgrade, a
    // 422 + unknown cause would become `RpcError::Rpc { code: -32000 }`,
    // which `is_retryable()` treats as retryable — causing extra retry
    // loops for deterministic client-side failures.
    // ========================================================================

    #[test]
    fn test_non_2xx_unknown_cause_4xx_downgrades_to_network() {
        // 422 with a fictional unknown handler cause. nearcore (or a gateway)
        // may introduce new cause names we don't yet map — those must retain
        // the pre-decode classification (non-retryable on 4xx).
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "cause": {
                    "name": "FUTURE_HANDLER_ERROR",
                    "info": {}
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let parsed_err = client.parse_rpc_error(&error);
        // Sanity check: the parse step itself returns the generic Rpc variant
        // because the cause name isn't mapped. The downgrade is what fixes it.
        assert!(matches!(parsed_err, RpcError::Rpc { .. }));

        let result = preserve_http_retry_classification(parsed_err, 422, body);
        match result {
            RpcError::Network {
                status_code,
                retryable,
                ..
            } => {
                assert_eq!(status_code, Some(422));
                assert!(!retryable, "4xx must never be retryable");
            }
            _ => panic!(
                "expected Network error for 4xx + unknown cause, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_non_2xx_unknown_cause_418_downgrades_to_network() {
        // Same principle on a different 4xx code — 4xx is 4xx regardless of
        // which specific code the upstream returns.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "I'm a teapot",
                "cause": {
                    "name": "SOME_UNMAPPED_CAUSE",
                    "info": {}
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let parsed_err = client.parse_rpc_error(&error);
        let result = preserve_http_retry_classification(parsed_err, 418, body);
        match result {
            RpcError::Network {
                status_code,
                retryable,
                ..
            } => {
                assert_eq!(status_code, Some(418));
                assert!(!retryable, "4xx must never be retryable");
            }
            _ => panic!(
                "expected Network error for 4xx + unknown cause, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_non_2xx_typed_variant_passes_through_unchanged() {
        // Typed variants (UnknownBlock, RequestTimeout, etc.) have well-known
        // retry semantics and must not be downgraded — even on a 4xx status.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "data": "DB Not Found Error: BLOCK HEIGHT: 1",
                "cause": {
                    "name": "UNKNOWN_BLOCK",
                    "info": {}
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let parsed_err = client.parse_rpc_error(&error);
        let result = preserve_http_retry_classification(parsed_err, 422, body);
        assert!(
            matches!(result, RpcError::UnknownBlock(_)),
            "typed UnknownBlock must pass through, got {:?}",
            result
        );
    }

    #[test]
    fn test_non_2xx_unknown_cause_5xx_left_as_rpc() {
        // 5xx + unknown cause is plausibly a transient server-side issue; keep
        // the generic Rpc variant so existing retry semantics still apply.
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Server error",
                "cause": {
                    "name": "FUTURE_HANDLER_ERROR",
                    "info": {}
                },
                "name": "HANDLER_ERROR"
            }
        }"#;
        let parsed: JsonRpcResponse = serde_json::from_str(body).expect("valid envelope");
        let error = parsed.error.expect("error envelope present");
        let client = RpcClient::new("https://example.com");
        let parsed_err = client.parse_rpc_error(&error);
        let result = preserve_http_retry_classification(parsed_err, 503, body);
        assert!(
            matches!(result, RpcError::Rpc { .. }),
            "5xx + unknown cause should remain Rpc, got {:?}",
            result
        );
    }

    // ========================================================================
    // block_id_or_null tests
    // ========================================================================

    #[test]
    fn test_block_id_or_null_with_none() {
        let result = block_id_or_null(None);
        assert!(result.is_null());
    }

    #[test]
    fn test_block_id_or_null_with_height() {
        let block = BlockReference::at_height(12345);
        let result = block_id_or_null(Some(&block));
        assert_eq!(result, serde_json::json!(12345));
    }

    #[test]
    fn test_block_id_or_null_with_hash() {
        let hash = CryptoHash::hash(b"test block");
        let block = BlockReference::at_hash(hash);
        let result = block_id_or_null(Some(&block));
        assert_eq!(result, serde_json::json!(hash.to_string()));
    }

    #[test]
    fn test_block_id_or_null_with_finality_falls_back_to_null() {
        let block = BlockReference::final_();
        let result = block_id_or_null(Some(&block));
        assert!(result.is_null(), "finality variants should map to null");

        let block = BlockReference::optimistic();
        let result = block_id_or_null(Some(&block));
        assert!(result.is_null(), "optimistic should map to null");
    }

    #[test]
    fn test_block_id_or_null_with_sync_checkpoint_falls_back_to_null() {
        let block = BlockReference::genesis();
        let result = block_id_or_null(Some(&block));
        assert!(result.is_null(), "sync checkpoint should map to null");
    }

    // ========================================================================
    // Tracing event tests
    // ========================================================================

    /// The library must not log above DEBUG for errors it hands back to the
    /// caller. These install a capturing subscriber and check the level and
    /// fields of every event `RpcClient::call` emits.
    #[cfg(feature = "tracing")]
    mod tracing_events {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        use tracing::field::{Field, Visit};
        use tracing::{Event, Level, Subscriber};
        use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

        use super::*;

        /// A captured event: its level plus every field rendered as text.
        #[derive(Clone, Debug)]
        struct CapturedEvent {
            level: Level,
            fields: Vec<(&'static str, String)>,
        }

        impl CapturedEvent {
            fn field(&self, name: &str) -> Option<&str> {
                self.fields
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.as_str())
            }

            fn message(&self) -> Option<&str> {
                self.field("message")
            }
        }

        struct FieldVisitor<'a>(&'a mut Vec<(&'static str, String)>);

        impl Visit for FieldVisitor<'_> {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.push((field.name(), format!("{value:?}")));
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.push((field.name(), value.to_string()));
            }
        }

        #[derive(Clone, Default)]
        struct EventLog(Arc<Mutex<Vec<CapturedEvent>>>);

        impl<S: Subscriber> Layer<S> for EventLog {
            fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
                let mut fields = Vec::new();
                event.record(&mut FieldVisitor(&mut fields));
                self.0.lock().unwrap().push(CapturedEvent {
                    level: *event.metadata().level(),
                    fields,
                });
            }
        }

        impl EventLog {
            /// Install as the thread-default subscriber for the current test.
            fn install(&self) -> tracing::subscriber::DefaultGuard {
                tracing::subscriber::set_default(tracing_subscriber::registry().with(self.clone()))
            }

            fn events(&self) -> Vec<CapturedEvent> {
                self.0.lock().unwrap().clone()
            }
        }

        fn assert_nothing_above_debug(events: &[CapturedEvent]) {
            let loud: Vec<_> = events
                .iter()
                .filter(|e| matches!(e.level, Level::INFO | Level::WARN | Level::ERROR))
                .collect();
            assert!(
                loud.is_empty(),
                "expected no events above DEBUG, got: {loud:?}"
            );
        }

        /// Fails the first request with a retryable HTTP status, then succeeds.
        struct FlakyTransport {
            calls: AtomicUsize,
        }

        impl RpcTransport for FlakyTransport {
            fn post_json(
                &self,
                _url: &str,
                _body: Vec<u8>,
            ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
                let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    Ok(if attempt == 0 {
                        TransportResponse {
                            status: 503,
                            body: b"upstream unavailable".to_vec(),
                        }
                    } else {
                        TransportResponse {
                            status: 200,
                            body: br#"{"jsonrpc":"2.0","id":0,"result":{"ok":true}}"#.to_vec(),
                        }
                    })
                })
            }
        }

        #[tokio::test]
        async fn terminal_error_is_debug_and_names_the_variant() {
            let log = EventLog::default();
            let _guard = log.install();

            // The EXPERIMENTAL endpoint omits contract_id/method_name, so the
            // parser produces `unknown::unknown` and `view_function` patches in
            // the real names afterwards.
            let contract_id: AccountId = "contract.near".parse().unwrap();
            let error = rpc_with_handler_error(
                "CONTRACT_EXECUTION_ERROR",
                serde_json::json!({ "vm_error": { "MethodResolveError": "MethodNotFound" } }),
            )
            .view_function(
                &contract_id,
                "no_such_method",
                &[],
                BlockReference::final_(),
            )
            .await
            .unwrap_err();
            assert!(
                matches!(&error, RpcError::MethodNotFound { method_name, .. } if method_name == "no_such_method"),
                "expected enriched MethodNotFound, got {error:?}"
            );

            let events = log.events();
            assert_nothing_above_debug(&events);

            let failed = events
                .iter()
                .find(|e| e.message() == Some("RPC request failed"))
                .unwrap_or_else(|| panic!("no terminal failure event in {events:?}"));
            assert_eq!(failed.level, Level::DEBUG);
            assert_eq!(failed.field("error.kind"), Some("MethodNotFound"));

            // The pre-enrichment `Display` text must not leak into any event.
            let leaked: Vec<_> = events
                .iter()
                .filter(|e| e.fields.iter().any(|(_, v)| v.contains("unknown::unknown")))
                .collect();
            assert!(
                leaked.is_empty(),
                "pre-enrichment error text leaked: {leaked:?}"
            );
        }

        #[tokio::test]
        async fn retry_is_debug() {
            let log = EventLog::default();
            let _guard = log.install();

            let transport = Arc::new(FlakyTransport {
                calls: AtomicUsize::new(0),
            });
            let client = RpcClient::with_transport_and_retry_config(
                "https://example.com",
                transport.clone(),
                RetryConfig {
                    max_retries: 1,
                    initial_delay_ms: 0,
                    max_delay_ms: 0,
                },
            );

            let result: serde_json::Value =
                client.call("status", serde_json::json!({})).await.unwrap();
            assert_eq!(result["ok"], true);
            assert_eq!(transport.calls.load(Ordering::SeqCst), 2);

            let events = log.events();
            assert_nothing_above_debug(&events);

            let retry = events
                .iter()
                .find(|e| e.message() == Some("RPC request failed, retrying"))
                .unwrap_or_else(|| panic!("no retry event in {events:?}"));
            assert_eq!(retry.level, Level::DEBUG);
            assert_eq!(retry.field("attempt"), Some("1"));
            assert_eq!(retry.field("max_attempts"), Some("2"));
        }
    }

    // ========================================================================
    // Block metadata on typed `query` views
    // ========================================================================

    fn rpc_with_result(result: serde_json::Value) -> RpcClient {
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": result,
        }))
        .unwrap();
        RpcClient::with_transport_and_retry_config(
            "https://example.com",
            Arc::new(StaticResponseTransport { body }),
            RetryConfig {
                max_retries: 0,
                ..RetryConfig::default()
            },
        )
    }

    const QUERY_BLOCK_HASH: &str = "H33oNAtVZDJjhpncQb5LY6NxYzQLMMVLptq99mwmLmnj";

    #[tokio::test]
    async fn test_view_code_keeps_block_metadata() {
        let code_hash = CryptoHash::hash(b"\0asm");
        let rpc = rpc_with_result(serde_json::json!({
            "code_base64": STANDARD.encode(b"\0asm"),
            "hash": code_hash.to_string(),
            "block_height": 42u64,
            "block_hash": QUERY_BLOCK_HASH,
        }));
        let account: AccountId = "app.near".parse().unwrap();

        let view = rpc
            .view_code(&account, BlockReference::final_())
            .await
            .unwrap();
        assert_eq!(view.code, b"\0asm");
        assert_eq!(view.hash, code_hash);
        assert_eq!(view.block_height, 42);
        assert_eq!(view.block_hash, QUERY_BLOCK_HASH.parse().unwrap());

        // Same wire shape for the global-contract lookups.
        let id = GlobalContractId::CodeHash(*code_hash.as_bytes());
        let global = rpc
            .view_global_contract_code(&id, BlockReference::final_())
            .await
            .unwrap();
        assert_eq!(global, view);
    }

    #[tokio::test]
    async fn test_view_state_keeps_block_metadata() {
        let rpc = rpc_with_result(serde_json::json!({
            "values": [{ "key": STANDARD.encode(b"k"), "value": STANDARD.encode(b"v") }],
            "block_height": 9u64,
            "block_hash": QUERY_BLOCK_HASH,
        }));
        let account: AccountId = "app.near".parse().unwrap();

        let page = rpc
            .view_state(&account, b"", None, None, BlockReference::final_())
            .await
            .unwrap();
        assert_eq!(page.values.len(), 1);
        assert_eq!(page.values[0].key, b"k");
        assert_eq!(page.last_key, None);
        assert_eq!(page.block_height, 9);
        assert_eq!(page.block_hash, QUERY_BLOCK_HASH.parse().unwrap());
    }
}
