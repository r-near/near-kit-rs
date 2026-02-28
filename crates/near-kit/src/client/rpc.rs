//! Low-level JSON-RPC client for NEAR.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::RpcError;
use crate::types::{
    AccessKeyListView, AccessKeyView, AccountId, AccountView, BlockReference, BlockView,
    CryptoHash, GasPrice, PublicKey, SendTxResponse, SendTxWithReceiptsResponse, SignedTransaction,
    StatusResponse, TxExecutionStatus, ViewFunctionResult,
};

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
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retries.
    pub max_retries: u32,
    /// Initial delay in milliseconds.
    pub initial_delay_ms: u64,
    /// Maximum delay in milliseconds.
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
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
#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<T>,
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

/// Query response for view function calls.
/// NEAR RPC returns `result` on success or `error` on failure.
#[derive(Debug, Deserialize)]
struct QueryResponse {
    #[serde(default)]
    result: Option<Vec<u8>>,
    #[serde(default)]
    logs: Vec<String>,
    block_height: u64,
    block_hash: CryptoHash,
    #[serde(default)]
    error: Option<String>,
}

/// Low-level JSON-RPC client for NEAR.
pub struct RpcClient {
    url: String,
    client: reqwest::Client,
    retry_config: RetryConfig,
    request_id: AtomicU64,
}

impl RpcClient {
    /// Create a new RPC client with the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
            retry_config: RetryConfig::default(),
            request_id: AtomicU64::new(0),
        }
    }

    /// Create a new RPC client with custom retry configuration.
    pub fn with_retry_config(url: impl Into<String>, retry_config: RetryConfig) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
            retry_config,
            request_id: AtomicU64::new(0),
        }
    }

    /// Get the RPC URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Make a raw RPC call with retries.
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
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(RpcError::Timeout(total_attempts))
    }

    /// Single attempt to make an RPC call.
    async fn try_call<R: DeserializeOwned>(
        &self,
        request: &JsonRpcRequest<'_, impl Serialize>,
    ) -> Result<R, RpcError> {
        let response = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            let retryable = is_retryable_status(status.as_u16());
            return Err(RpcError::network(
                format!("HTTP {}: {}", status, body),
                Some(status.as_u16()),
                retryable,
            ));
        }

        let rpc_response: JsonRpcResponse<R> =
            serde_json::from_str(&body).map_err(RpcError::Json)?;

        if let Some(error) = rpc_response.error {
            return Err(self.parse_rpc_error(&error));
        }

        rpc_response
            .result
            .ok_or_else(|| RpcError::InvalidResponse("Missing result in response".to_string()))
    }

    /// Parse an RPC error into a specific error type.
    fn parse_rpc_error(&self, error: &JsonRpcError) -> RpcError {
        // First, check the direct cause field (NEAR RPC structured errors)
        if let Some(cause) = &error.cause {
            let cause_name = cause.name.as_str();
            let info = cause.info.as_ref();
            let data = &error.data;

            match cause_name {
                "UNKNOWN_ACCOUNT" => {
                    if let Some(account_id) = info
                        .and_then(|i| i.get("requested_account_id"))
                        .and_then(|a| a.as_str())
                    {
                        if let Ok(account_id) = account_id.parse() {
                            return RpcError::AccountNotFound(account_id);
                        }
                    }
                }
                "INVALID_ACCOUNT" => {
                    let account_id = info
                        .and_then(|i| i.get("requested_account_id"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("unknown");
                    return RpcError::InvalidAccount(account_id.to_string());
                }
                "UNKNOWN_ACCESS_KEY" => {
                    if let (Some(account_id), Some(public_key)) = (
                        info.and_then(|i| i.get("requested_account_id"))
                            .and_then(|a| a.as_str()),
                        info.and_then(|i| i.get("public_key"))
                            .and_then(|k| k.as_str()),
                    ) {
                        if let (Ok(account_id), Ok(public_key)) =
                            (account_id.parse(), public_key.parse())
                        {
                            return RpcError::AccessKeyNotFound {
                                account_id,
                                public_key,
                            };
                        }
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
                        return RpcError::ContractNotDeployed(account_id);
                    }
                }
                "TOO_LARGE_CONTRACT_STATE" => {
                    let account_id = info
                        .and_then(|i| i.get("account_id").or_else(|| i.get("contract_id")))
                        .and_then(|a| a.as_str())
                        .unwrap_or("unknown");
                    if let Ok(account_id) = account_id.parse() {
                        return RpcError::ContractStateTooLarge(account_id);
                    }
                }
                "CONTRACT_EXECUTION_ERROR" => {
                    let contract_id = info
                        .and_then(|i| i.get("contract_id"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown");
                    let method_name = info
                        .and_then(|i| i.get("method_name"))
                        .and_then(|m| m.as_str())
                        .map(String::from);
                    if let Ok(contract_id) = contract_id.parse() {
                        return RpcError::ContractExecution {
                            contract_id,
                            method_name,
                            message: error.message.clone(),
                        };
                    }
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
                    if let Some(invalid_nonce) = data.as_ref().and_then(extract_invalid_nonce) {
                        return invalid_nonce;
                    }
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
        if let Some(data) = &error.data {
            if let Some(error_str) = data.as_str() {
                if error_str.contains("does not exist") {
                    // Try to extract account ID from error message
                    // Format: "account X does not exist while viewing"
                    if let Some(start) = error_str.strip_prefix("account ") {
                        if let Some(account_str) = start.split_whitespace().next() {
                            if let Ok(account_id) = account_str.parse() {
                                return RpcError::AccountNotFound(account_id);
                            }
                        }
                    }
                }
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
    pub async fn view_account(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<AccountView, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_account",
            "account_id": account_id.to_string(),
        });

        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await
    }

    /// View access key information.
    pub async fn view_access_key(
        &self,
        account_id: &AccountId,
        public_key: &PublicKey,
        block: BlockReference,
    ) -> Result<AccessKeyView, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_access_key",
            "account_id": account_id.to_string(),
            "public_key": public_key.to_string(),
        });

        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await
    }

    /// View all access keys for an account.
    pub async fn view_access_key_list(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<AccessKeyListView, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "view_access_key_list",
            "account_id": account_id.to_string(),
        });

        self.merge_block_reference(&mut params, &block);
        self.call("query", params).await
    }

    /// Call a view function on a contract.
    pub async fn view_function(
        &self,
        account_id: &AccountId,
        method_name: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, RpcError> {
        let mut params = serde_json::json!({
            "request_type": "call_function",
            "account_id": account_id.to_string(),
            "method_name": method_name,
            "args_base64": STANDARD.encode(args),
        });

        self.merge_block_reference(&mut params, &block);

        // Query responses may have an error field instead of result
        let response: QueryResponse = self.call("query", params).await?;

        if let Some(error) = response.error {
            // Parse the error message for known patterns
            if error.contains("CodeDoesNotExist") {
                return Err(RpcError::ContractNotDeployed(account_id.clone()));
            }
            if error.contains("MethodNotFound") || error.contains("MethodResolveError") {
                return Err(RpcError::ContractExecution {
                    contract_id: account_id.clone(),
                    method_name: Some(method_name.to_string()),
                    message: error,
                });
            }
            return Err(RpcError::ContractExecution {
                contract_id: account_id.clone(),
                method_name: Some(method_name.to_string()),
                message: error,
            });
        }

        Ok(ViewFunctionResult {
            result: response.result.unwrap_or_default(),
            logs: response.logs,
            block_height: response.block_height,
            block_hash: response.block_hash,
        })
    }

    /// Get block information.
    pub async fn block(&self, block: BlockReference) -> Result<BlockView, RpcError> {
        let params = block.to_rpc_params();
        self.call("block", params).await
    }

    /// Get node status.
    pub async fn status(&self) -> Result<StatusResponse, RpcError> {
        self.call("status", serde_json::json!([])).await
    }

    /// Get current gas price.
    pub async fn gas_price(&self, block_hash: Option<&CryptoHash>) -> Result<GasPrice, RpcError> {
        let params = match block_hash {
            Some(hash) => serde_json::json!([hash.to_string()]),
            None => serde_json::json!([serde_json::Value::Null]),
        };
        self.call("gas_price", params).await
    }

    /// Send a signed transaction.
    pub async fn send_tx(
        &self,
        signed_tx: &SignedTransaction,
        wait_until: TxExecutionStatus,
    ) -> Result<SendTxResponse, RpcError> {
        let params = serde_json::json!({
            "signed_tx_base64": signed_tx.to_base64(),
            "wait_until": wait_until.as_str(),
        });
        self.call("send_tx", params).await
    }

    /// Get transaction status with full receipt details.
    ///
    /// Uses EXPERIMENTAL_tx_status which returns complete receipt information.
    pub async fn tx_status(
        &self,
        tx_hash: &CryptoHash,
        sender_id: &AccountId,
        wait_until: TxExecutionStatus,
    ) -> Result<SendTxWithReceiptsResponse, RpcError> {
        let params = serde_json::json!({
            "tx_hash": tx_hash.to_string(),
            "sender_account_id": sender_id.to_string(),
            "wait_until": wait_until.as_str(),
        });
        self.call("EXPERIMENTAL_tx_status", params).await
    }

    /// Merge block reference parameters into a JSON object.
    fn merge_block_reference(&self, params: &mut serde_json::Value, block: &BlockReference) {
        if let serde_json::Value::Object(block_params) = block.to_rpc_params() {
            if let serde_json::Value::Object(map) = params {
                map.extend(block_params);
            }
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
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }
}

impl Clone for RpcClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            client: self.client.clone(),
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

/// Check if an HTTP status code is retryable.
fn is_retryable_status(status: u16) -> bool {
    // 408 Request Timeout - retryable
    // 429 Too Many Requests - retryable (rate limiting)
    // 503 Service Unavailable - retryable
    // 5xx Server Errors - retryable
    status == 408 || status == 429 || status == 503 || (500..600).contains(&status)
}

/// Extract InvalidNonce error from data.
fn extract_invalid_nonce(data: &serde_json::Value) -> Option<RpcError> {
    // Navigate nested error structure: TxExecutionError.InvalidTxError.InvalidNonce
    let tx_exec_error = data.get("TxExecutionError")?;
    let invalid_tx_error = tx_exec_error
        .get("InvalidTxError")
        .or_else(|| data.get("InvalidTxError"))?;
    let invalid_nonce = invalid_tx_error.get("InvalidNonce")?;

    let ak_nonce = invalid_nonce.get("ak_nonce")?.as_u64()?;
    let tx_nonce = invalid_nonce.get("tx_nonce")?.as_u64()?;

    Some(RpcError::InvalidNonce { tx_nonce, ak_nonce })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    // extract_invalid_nonce tests
    // ========================================================================

    #[test]
    fn test_extract_invalid_nonce_success() {
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
        let result = extract_invalid_nonce(&data);
        assert!(result.is_some());
        match result.unwrap() {
            RpcError::InvalidNonce { tx_nonce, ak_nonce } => {
                assert_eq!(tx_nonce, 5);
                assert_eq!(ak_nonce, 10);
            }
            _ => panic!("Expected InvalidNonce error"),
        }
    }

    #[test]
    fn test_extract_invalid_nonce_missing_fields() {
        // Missing TxExecutionError
        let data = serde_json::json!({
            "SomeOtherError": {}
        });
        assert!(extract_invalid_nonce(&data).is_none());

        // Missing InvalidTxError
        let data = serde_json::json!({
            "TxExecutionError": {
                "SomeOtherError": {}
            }
        });
        assert!(extract_invalid_nonce(&data).is_none());

        // Missing InvalidNonce
        let data = serde_json::json!({
            "TxExecutionError": {
                "InvalidTxError": {
                    "SomeOtherError": {}
                }
            }
        });
        assert!(extract_invalid_nonce(&data).is_none());

        // Missing tx_nonce
        let data = serde_json::json!({
            "TxExecutionError": {
                "InvalidTxError": {
                    "InvalidNonce": {
                        "ak_nonce": 10
                    }
                }
            }
        });
        assert!(extract_invalid_nonce(&data).is_none());

        // Missing ak_nonce
        let data = serde_json::json!({
            "TxExecutionError": {
                "InvalidTxError": {
                    "InvalidNonce": {
                        "tx_nonce": 5
                    }
                }
            }
        });
        assert!(extract_invalid_nonce(&data).is_none());
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
                    "requested_account_id": "nonexistent.near"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::AccountNotFound(_)));
    }

    #[test]
    fn test_parse_rpc_error_unknown_access_key() {
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
            } => {
                assert_eq!(account_id.as_ref(), "alice.near");
                assert!(public_key.to_string().contains("ed25519:"));
            }
            _ => panic!("Expected AccessKeyNotFound error, got {:?}", result),
        }
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
                    "requested_account_id": "invalid@account"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::InvalidAccount(_)));
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
                    "contract_account_id": "no-contract.near"
                })),
            }),
            name: None,
        };
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::ContractNotDeployed(_)));
    }

    #[test]
    fn test_parse_rpc_error_too_large_contract_state() {
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
        let result = client.parse_rpc_error(&error);
        assert!(matches!(result, RpcError::ContractStateTooLarge(_)));
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
    fn test_parse_rpc_error_contract_execution() {
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
        assert!(matches!(result, RpcError::ContractExecution { .. }));
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
        assert!(matches!(result, RpcError::AccountNotFound(_)));
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
}
