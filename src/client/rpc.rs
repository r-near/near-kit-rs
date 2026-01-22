//! Low-level JSON-RPC client for NEAR.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::RpcError;
use crate::types::{
    AccessKeyListView, AccessKeyView, AccountId, AccountView, BlockReference, BlockView,
    CryptoHash, FinalExecutionOutcome, GasPrice, PublicKey, SignedTransaction,
    StatusResponse, TxExecutionStatus, ViewFunctionResult,
};

/// Network configuration presets.
pub struct NetworkConfig {
    pub rpc_url: &'static str,
    pub network_id: &'static str,
}

/// Mainnet configuration.
pub const MAINNET: NetworkConfig = NetworkConfig {
    rpc_url: "https://rpc.mainnet.near.org",
    network_id: "mainnet",
};

/// Testnet configuration.
pub const TESTNET: NetworkConfig = NetworkConfig {
    rpc_url: "https://rpc.testnet.near.org",
    network_id: "testnet",
};

/// Localnet configuration.
pub const LOCALNET: NetworkConfig = NetworkConfig {
    rpc_url: "http://localhost:3030",
    network_id: "localnet",
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
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
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
            return Err(RpcError::InvalidResponse(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        let rpc_response: JsonRpcResponse<R> =
            serde_json::from_str(&body).map_err(|e| RpcError::Json(e))?;

        if let Some(error) = rpc_response.error {
            return Err(self.parse_rpc_error(&error));
        }

        rpc_response
            .result
            .ok_or_else(|| RpcError::InvalidResponse("Missing result in response".to_string()))
    }

    /// Parse an RPC error into a specific error type.
    fn parse_rpc_error(&self, error: &JsonRpcError) -> RpcError {
        // Try to extract structured error info
        if let Some(data) = &error.data {
            if let Some(cause) = data
                .get("cause")
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
            {
                match cause {
                    "UNKNOWN_ACCOUNT" => {
                        if let Some(account_id) = data
                            .get("cause")
                            .and_then(|c| c.get("info"))
                            .and_then(|i| i.get("account_id"))
                            .and_then(|a| a.as_str())
                        {
                            if let Ok(account_id) = account_id.parse() {
                                return RpcError::AccountNotFound(account_id);
                            }
                        }
                    }
                    "UNKNOWN_ACCESS_KEY" => {
                        // TODO: Extract account_id and public_key
                    }
                    _ => {}
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
        self.call("query", params).await
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
    ) -> Result<FinalExecutionOutcome, RpcError> {
        let params = serde_json::json!({
            "signed_tx_base64": signed_tx.to_base64(),
            "wait_until": wait_until.as_str(),
        });
        self.call("send_tx", params).await
    }

    /// Get transaction status.
    pub async fn tx_status(
        &self,
        tx_hash: &CryptoHash,
        sender_id: &AccountId,
        wait_until: TxExecutionStatus,
    ) -> Result<FinalExecutionOutcome, RpcError> {
        let params = serde_json::json!({
            "tx_hash": tx_hash.to_string(),
            "sender_account_id": sender_id.to_string(),
            "wait_until": wait_until.as_str(),
        });
        self.call("tx", params).await
    }

    /// Merge block reference parameters into a JSON object.
    fn merge_block_reference(&self, params: &mut serde_json::Value, block: &BlockReference) {
        if let serde_json::Value::Object(block_params) = block.to_rpc_params() {
            if let serde_json::Value::Object(ref mut map) = params {
                map.extend(block_params);
            }
        }
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
