//! The main Near client.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::contract::ContractClient;
use crate::error::Error;
use crate::types::{AccountId, Gas, IntoNearToken, NearToken, PublicKey, SecretKey};

use super::query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
use super::rpc::{RetryConfig, RpcClient, MAINNET, TESTNET};
use super::signer::{InMemorySigner, Signer};
use super::transaction::TransactionBuilder;
use super::tx::{AddKeyCall, ContractCall, DeleteKeyCall, DeployCall, TransferCall};

/// Trait for sandbox network configuration.
///
/// Implement this trait for your sandbox type to enable ergonomic
/// integration with the `Near` client via [`Near::sandbox()`].
///
/// # Example
///
/// ```rust,ignore
/// use near_sandbox::Sandbox;
///
/// let sandbox = Sandbox::start_sandbox().await?;
/// let near = Near::sandbox(&sandbox).build();
///
/// // The root account credentials are automatically configured
/// near.transfer("alice.sandbox", "10 NEAR").await?;
/// ```
pub trait SandboxNetwork {
    /// The RPC URL for the sandbox (e.g., `http://127.0.0.1:3030`).
    fn rpc_url(&self) -> &str;

    /// The root account ID (e.g., `"sandbox"`).
    fn root_account_id(&self) -> &str;

    /// The root account's secret key.
    fn root_secret_key(&self) -> &str;
}

/// The main client for interacting with NEAR Protocol.
///
/// The `Near` client is the single entry point for all NEAR operations.
/// It can be configured with a signer for write operations, or used
/// without a signer for read-only operations.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
///
/// #[tokio::main]
/// async fn main() -> Result<(), near_kit::Error> {
///     // Read-only client (no signer)
///     let near = Near::testnet().build();
///     let balance = near.balance("alice.testnet").await?;
///     println!("Balance: {}", balance);
///     
///     // Client with signer for transactions
///     let near = Near::testnet()
///         .credentials("ed25519:...", "alice.testnet")?
///         .build();
///     near.transfer("bob.testnet", "1 NEAR").await?;
///     
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Near {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
}

impl Near {
    /// Create a builder for mainnet.
    pub fn mainnet() -> NearBuilder {
        NearBuilder::new(MAINNET.rpc_url)
    }

    /// Create a builder for testnet.
    pub fn testnet() -> NearBuilder {
        NearBuilder::new(TESTNET.rpc_url)
    }

    /// Create a builder with a custom RPC URL.
    pub fn custom(rpc_url: impl Into<String>) -> NearBuilder {
        NearBuilder::new(rpc_url)
    }

    /// Create a builder configured for a sandbox network.
    ///
    /// This automatically configures the client with the sandbox's RPC URL
    /// and root account credentials, making it ready for transactions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_sandbox::Sandbox;
    /// use near_kit::*;
    ///
    /// let sandbox = Sandbox::start_sandbox().await?;
    /// let near = Near::sandbox(&sandbox);
    ///
    /// // Root account credentials are auto-configured - ready for transactions!
    /// near.transfer("alice.sandbox", "10 NEAR").await?;
    /// ```
    pub fn sandbox(network: &impl SandboxNetwork) -> Near {
        let secret_key: SecretKey = network
            .root_secret_key()
            .parse()
            .expect("sandbox should provide valid secret key");
        let account_id: AccountId = network
            .root_account_id()
            .parse()
            .expect("sandbox should provide valid account id");

        let signer = InMemorySigner::from_secret_key(account_id, secret_key);

        Near {
            rpc: Arc::new(RpcClient::new(network.rpc_url())),
            signer: Some(Arc::new(signer)),
        }
    }

    /// Get the underlying RPC client.
    pub fn rpc(&self) -> &RpcClient {
        &self.rpc
    }

    /// Get the RPC URL.
    pub fn rpc_url(&self) -> &str {
        self.rpc.url()
    }

    /// Get the signer's account ID, if a signer is configured.
    pub fn account_id(&self) -> Option<&AccountId> {
        self.signer.as_ref().map(|s| s.account_id())
    }

    // ========================================================================
    // Read Operations (Query Builders)
    // ========================================================================

    /// Get account balance.
    ///
    /// Returns a query builder that can be customized with block reference
    /// options before awaiting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// // Simple query
    /// let balance = near.balance("alice.testnet").await?;
    /// println!("Available: {}", balance.available);
    ///
    /// // Query at specific block height
    /// let balance = near.balance("alice.testnet")
    ///     .at_block(100_000_000)
    ///     .await?;
    ///
    /// // Query with specific finality
    /// let balance = near.balance("alice.testnet")
    ///     .finality(Finality::Optimistic)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn balance(&self, account_id: impl AsRef<str>) -> BalanceQuery {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        BalanceQuery::new(self.rpc.clone(), account_id)
    }

    /// Get full account information.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let account = near.account("alice.testnet").await?;
    /// println!("Storage used: {} bytes", account.storage_usage);
    /// # Ok(())
    /// # }
    /// ```
    pub fn account(&self, account_id: impl AsRef<str>) -> AccountQuery {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        AccountQuery::new(self.rpc.clone(), account_id)
    }

    /// Check if an account exists.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// if near.account_exists("alice.testnet").await? {
    ///     println!("Account exists!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn account_exists(&self, account_id: impl AsRef<str>) -> AccountExistsQuery {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        AccountExistsQuery::new(self.rpc.clone(), account_id)
    }

    /// Call a view function on a contract.
    ///
    /// Returns a query builder that can be customized with arguments
    /// and block reference options before awaiting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// // Simple view call
    /// let count: u64 = near.view("counter.testnet", "get_count").await?;
    ///
    /// // View call with arguments
    /// let messages: Vec<String> = near.view("guestbook.testnet", "get_messages")
    ///     .args(serde_json::json!({ "limit": 10 }))
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn view<T: DeserializeOwned + Send + 'static>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
    ) -> ViewCall<T> {
        let contract_id = contract_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(contract_id.as_ref()));
        ViewCall::new(self.rpc.clone(), contract_id, method.to_string())
    }

    /// Get all access keys for an account.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let keys = near.access_keys("alice.testnet").await?;
    /// for key_info in keys.keys {
    ///     println!("Key: {}", key_info.public_key);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn access_keys(&self, account_id: impl AsRef<str>) -> AccessKeysQuery {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        AccessKeysQuery::new(self.rpc.clone(), account_id)
    }

    // ========================================================================
    // Write Operations (Transaction Builders)
    // ========================================================================

    /// Transfer NEAR tokens.
    ///
    /// Returns a transaction builder that can be customized with
    /// wait options before awaiting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///         .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// // Preferred: typed constructor
    /// near.transfer("bob.testnet", NearToken::near(1)).await?;
    ///
    /// // Transfer with wait for finality
    /// near.transfer("bob.testnet", NearToken::near(1000))
    ///     .wait_until(TxExecutionStatus::Final)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(&self, receiver: impl AsRef<str>, amount: impl IntoNearToken) -> TransferCall {
        let receiver_id = receiver
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(receiver.as_ref()));
        let amount = amount.into_near_token().unwrap_or(NearToken::ZERO);
        TransferCall::new(self.rpc.clone(), self.signer.clone(), receiver_id, amount)
    }

    /// Call a function on a contract.
    ///
    /// Returns a transaction builder that can be customized with
    /// arguments, gas, deposit, and other options before awaiting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///         .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// // Simple call
    /// near.call("counter.testnet", "increment").await?;
    ///
    /// // Call with args, gas, and deposit
    /// near.call("nft.testnet", "nft_mint")
    ///     .args(serde_json::json!({ "token_id": "1" }))
    ///     .gas("100 Tgas")
    ///     .deposit("0.1 NEAR")
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(&self, contract_id: impl AsRef<str>, method: &str) -> ContractCall {
        let contract_id = contract_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(contract_id.as_ref()));
        ContractCall::new(
            self.rpc.clone(),
            self.signer.clone(),
            contract_id,
            method.to_string(),
        )
    }

    /// Deploy a contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///         .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// let wasm_code = std::fs::read("contract.wasm").unwrap();
    /// near.deploy("alice.testnet", wasm_code).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn deploy(&self, account_id: impl AsRef<str>, code: Vec<u8>) -> DeployCall {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        DeployCall::new(self.rpc.clone(), self.signer.clone(), account_id, code)
    }

    /// Add a full access key to an account.
    pub fn add_full_access_key(
        &self,
        account_id: impl AsRef<str>,
        public_key: PublicKey,
    ) -> AddKeyCall {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        AddKeyCall::new(
            self.rpc.clone(),
            self.signer.clone(),
            account_id,
            public_key,
        )
    }

    /// Delete an access key from an account.
    pub fn delete_key(&self, account_id: impl AsRef<str>, public_key: PublicKey) -> DeleteKeyCall {
        let account_id = account_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(account_id.as_ref()));
        DeleteKeyCall::new(
            self.rpc.clone(),
            self.signer.clone(),
            account_id,
            public_key,
        )
    }

    // ========================================================================
    // Multi-Action Transactions
    // ========================================================================

    /// Create a transaction builder for multi-action transactions.
    ///
    /// This allows chaining multiple actions (transfers, function calls, account creation, etc.)
    /// into a single atomic transaction. All actions either succeed together or fail together.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// // Create a new sub-account with funding and a key
    /// let new_public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
    /// near.transaction("new.alice.testnet")
    ///     .create_account()
    ///     .transfer("5 NEAR")
    ///     .add_full_access_key(new_public_key)
    ///     .send()
    ///     .await?;
    ///
    /// // Multiple function calls in one transaction
    /// near.transaction("contract.testnet")
    ///     .call("method1")
    ///         .args(serde_json::json!({ "value": 1 }))
    ///     .call("method2")
    ///         .args(serde_json::json!({ "value": 2 }))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transaction(&self, receiver_id: impl AsRef<str>) -> TransactionBuilder {
        let receiver_id = receiver_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(receiver_id.as_ref()));
        TransactionBuilder::new(self.rpc.clone(), self.signer.clone(), receiver_id)
    }

    // ========================================================================
    // Convenience methods
    // ========================================================================

    /// Call a view function with arguments (convenience method).
    pub async fn view_with_args<T: DeserializeOwned + Send + 'static, A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
    ) -> Result<T, Error> {
        let contract_id = contract_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(contract_id.as_ref()));
        ViewCall::new(self.rpc.clone(), contract_id, method.to_string())
            .args(args)
            .await
    }

    /// Call a function with arguments (convenience method).
    pub async fn call_with_args<A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
    ) -> Result<crate::types::FinalExecutionOutcome, Error> {
        self.call(contract_id, method).args(args).await
    }

    /// Call a function with full options (convenience method).
    pub async fn call_with_options<A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
        gas: Gas,
        deposit: NearToken,
    ) -> Result<crate::types::FinalExecutionOutcome, Error> {
        self.call(contract_id, method)
            .args(args)
            .gas(gas)
            .deposit(deposit)
            .await
    }

    // ========================================================================
    // Typed Contract Interfaces
    // ========================================================================

    /// Create a typed contract client.
    ///
    /// This method creates a type-safe client for interacting with a contract,
    /// using the interface defined via the `#[near_kit::contract]` macro.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use near_kit::*;
    /// use serde::Serialize;
    ///
    /// #[near_kit::contract]
    /// pub trait Counter {
    ///     fn get_count(&self) -> u64;
    ///     
    ///     #[call]
    ///     fn increment(&mut self);
    ///     
    ///     #[call]
    ///     fn add(&mut self, args: AddArgs);
    /// }
    ///
    /// #[derive(Serialize)]
    /// pub struct AddArgs {
    ///     pub value: u64,
    /// }
    ///
    /// async fn example(near: &Near) -> Result<(), near_kit::Error> {
    ///     let counter = near.contract::<Counter>("counter.testnet");
    ///     
    ///     // View call - type-safe!
    ///     let count = counter.get_count().await?;
    ///     
    ///     // Change call - type-safe!
    ///     counter.increment().await?;
    ///     counter.add(AddArgs { value: 5 }).await?;
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub fn contract<T: crate::Contract + ?Sized>(
        &self,
        contract_id: impl AsRef<str>,
    ) -> T::Client<'_> {
        let contract_id: AccountId = contract_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(contract_id.as_ref()));
        T::Client::new(self, contract_id)
    }
}

impl std::fmt::Debug for Near {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Near")
            .field("rpc", &self.rpc)
            .field("account_id", &self.account_id())
            .finish()
    }
}

/// Builder for creating a [`Near`] client.
///
/// # Example
///
/// ```rust,ignore
/// use near_kit::*;
///
/// // Read-only client
/// let near = Near::testnet().build();
///
/// // Client with credentials (secret key + account)
/// let near = Near::testnet()
///     .credentials("ed25519:...", "alice.testnet")?
///     .build();
///
/// // Client with keystore
/// let keystore = std::sync::Arc::new(InMemoryKeyStore::new());
/// // ... add keys to keystore ...
/// let near = Near::testnet()
///     .keystore(keystore, "alice.testnet")?
///     .build();
/// ```
pub struct NearBuilder {
    rpc_url: String,
    signer: Option<Arc<dyn Signer>>,
    retry_config: RetryConfig,
}

impl NearBuilder {
    /// Create a new builder with the given RPC URL.
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            signer: None,
            retry_config: RetryConfig::default(),
        }
    }

    /// Set the signer for transactions.
    ///
    /// The signer determines which account will sign transactions.
    pub fn signer(mut self, signer: impl Signer + 'static) -> Self {
        self.signer = Some(Arc::new(signer));
        self
    }

    /// Set up signing using a private key string and account ID.
    ///
    /// This is a convenience method that creates an `InMemorySigner` for you.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::Near;
    ///
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    /// ```
    pub fn credentials(
        mut self,
        private_key: impl AsRef<str>,
        account_id: impl AsRef<str>,
    ) -> Result<Self, Error> {
        let signer = InMemorySigner::new(account_id, private_key)?;
        self.signer = Some(Arc::new(signer));
        Ok(self)
    }

    /// Set the retry configuration.
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Build the client.
    pub fn build(self) -> Near {
        Near {
            rpc: Arc::new(RpcClient::with_retry_config(
                self.rpc_url,
                self.retry_config,
            )),
            signer: self.signer,
        }
    }
}

impl From<NearBuilder> for Near {
    fn from(builder: NearBuilder) -> Self {
        builder.build()
    }
}

// ============================================================================
// near-sandbox integration (behind feature flag or for dev dependencies)
// ============================================================================

/// Default sandbox root account ID.
pub const SANDBOX_ROOT_ACCOUNT: &str = "sandbox";

/// Default sandbox root account private key.
pub const SANDBOX_ROOT_PRIVATE_KEY: &str =
    "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

#[cfg(feature = "sandbox")]
impl SandboxNetwork for near_sandbox::Sandbox {
    fn rpc_url(&self) -> &str {
        &self.rpc_addr
    }

    fn root_account_id(&self) -> &str {
        SANDBOX_ROOT_ACCOUNT
    }

    fn root_secret_key(&self) -> &str {
        SANDBOX_ROOT_PRIVATE_KEY
    }
}
