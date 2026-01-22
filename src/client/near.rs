//! The main Near client.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::types::{AccountId, Gas, NearToken, PublicKey};

use super::keystore::{InMemoryKeyStore, KeyStore};
use super::query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
use super::rpc::{RetryConfig, RpcClient, MAINNET, TESTNET};
use super::signer::{KeyStoreSigner, Signer};
use super::tx::{AddKeyCall, ContractCall, DeleteKeyCall, DeployCall, TransferCall};

/// The main client for interacting with NEAR Protocol.
///
/// The `Near` client is the single entry point for all NEAR operations.
/// It can be configured with a signer for write operations, or used
/// without a signer for read-only operations.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::prelude::*;
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

    /// Get the underlying RPC client.
    pub fn rpc(&self) -> &RpcClient {
        &self.rpc
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///         .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// // Simple transfer
    /// near.transfer("bob.testnet", "1 NEAR").await?;
    ///
    /// // Transfer with wait for finality
    /// near.transfer("bob.testnet", "1000 NEAR")
    ///     .wait_until(TxExecutionStatus::Final)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(&self, receiver: impl AsRef<str>, amount: impl AsRef<str>) -> TransferCall {
        let receiver_id = receiver
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(receiver.as_ref()));
        let amount: NearToken = amount.as_ref().parse().unwrap_or(NearToken::ZERO);
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
    /// # use near_kit::prelude::*;
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
    /// # use near_kit::prelude::*;
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
            .gas_amount(gas)
            .deposit_amount(deposit)
            .await
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
/// use near_kit::prelude::*;
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

    /// Set up signing using a keystore and account ID.
    ///
    /// This is a convenience method that creates a `KeyStoreSigner` for you.
    ///
    /// # Errors
    ///
    /// Returns an error if no key exists in the keystore for the given account.
    pub fn keystore(
        mut self,
        keystore: Arc<dyn KeyStore>,
        account_id: impl AsRef<str>,
    ) -> Result<Self, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let signer = KeyStoreSigner::new(keystore, account_id)?;
        self.signer = Some(Arc::new(signer));
        Ok(self)
    }

    /// Set up signing using a private key string and account ID.
    ///
    /// This is a convenience method that creates an in-memory keystore with the
    /// provided key and sets up a signer for the account.
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
        let secret_key: crate::types::SecretKey = private_key.as_ref().parse()?;
        let account_id: AccountId = account_id.as_ref().parse()?;

        // Create an in-memory keystore with this single key
        let keystore = Arc::new(InMemoryKeyStore::new());
        keystore.add(&account_id, secret_key);

        // Create a signer from the keystore
        let signer = KeyStoreSigner::new(keystore, account_id)?;
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
