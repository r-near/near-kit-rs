//! The main Near client.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::contract::ContractClient;
use crate::error::Error;
use crate::types::{
    AccountId, ChainId, Gas, GlobalContractRef, IntoNearToken, NearToken, PublicKey, PublishMode,
    SecretKey, TryIntoAccountId,
};

use super::query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
use super::rpc::{MAINNET, RetryConfig, RpcClient, TESTNET};
use super::signer::{InMemorySigner, Signer};
use super::transaction::{CallBuilder, TransactionBuilder};

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

    /// Optional chain ID override.
    ///
    /// If `None`, defaults to `"sandbox"`. Set this to mimic a specific
    /// network (e.g., `"mainnet"`) for chain-ID-dependent logic.
    fn chain_id(&self) -> Option<&str> {
        None
    }
}

/// The main client for interacting with NEAR Protocol.
///
/// The `Near` client is the single entry point for all NEAR operations.
/// It can be configured with a signer for write operations, or used
/// without a signer for read-only operations.
///
/// Transport (RPC connection) and signing are separate concerns — the client
/// holds a shared `Arc<RpcClient>` and an optional signer. Use [`with_signer`](Near::with_signer)
/// to derive new clients that share the same connection but sign as different accounts.
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
///
/// # Multiple Accounts
///
/// For production apps that manage multiple accounts, set up the connection once
/// and derive signing contexts with [`with_signer`](Near::with_signer):
///
/// ```rust,no_run
/// # use near_kit::*;
/// # fn example() -> Result<(), Error> {
/// let near = Near::testnet().build();
///
/// let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
/// let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);
///
/// // Both share the same RPC connection, sign as different accounts
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Near {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    chain_id: ChainId,
    max_nonce_retries: u32,
}

impl Near {
    /// Create a builder for mainnet.
    pub fn mainnet() -> NearBuilder {
        NearBuilder::new(MAINNET.rpc_url, ChainId::mainnet())
    }

    /// Create a builder for testnet.
    pub fn testnet() -> NearBuilder {
        NearBuilder::new(TESTNET.rpc_url, ChainId::testnet())
    }

    /// Create a builder with a custom RPC URL and chain ID.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::Near;
    ///
    /// // Private mainnet RPC
    /// let near = Near::custom("https://my-private-rpc.example.com", "mainnet").build();
    ///
    /// // Custom network
    /// let near = Near::custom("https://rpc.pinet.near.org", "pinet").build();
    /// ```
    pub fn custom(rpc_url: impl Into<String>, chain_id: impl Into<ChainId>) -> NearBuilder {
        NearBuilder::new(rpc_url, chain_id.into())
    }

    /// Create a configured client from environment variables.
    ///
    /// Reads the following environment variables:
    /// - `NEAR_NETWORK` (optional): `"mainnet"`, `"testnet"`, or a custom RPC URL.
    ///   Defaults to `"testnet"` if not set.
    /// - `NEAR_CHAIN_ID` (optional): Overrides the chain identifier (e.g., `"pinet"`).
    ///   If set, always overrides the chain ID inferred from `NEAR_NETWORK`, including
    ///   the built-in `"mainnet"` and `"testnet"` presets. Typically only needed for
    ///   custom networks.
    /// - `NEAR_ACCOUNT_ID` (optional): Account ID for signing transactions.
    /// - `NEAR_PRIVATE_KEY` (optional): Private key for signing (e.g., `"ed25519:..."`).
    /// - `NEAR_MAX_NONCE_RETRIES` (optional): Number of nonce retries on
    ///   `InvalidNonce` errors. `0` means no retries. Defaults to `3`.
    ///
    /// If `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` are both set, the client will
    /// be configured with signing capability. Otherwise, it will be read-only.
    ///
    /// # Example
    ///
    /// ```bash
    /// # Environment variables
    /// export NEAR_NETWORK=https://rpc.pinet.near.org
    /// export NEAR_CHAIN_ID=pinet
    /// export NEAR_ACCOUNT_ID=alice.testnet
    /// export NEAR_PRIVATE_KEY=ed25519:...
    /// export NEAR_MAX_NONCE_RETRIES=10
    /// ```
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// // Auto-configures from environment
    /// let near = Near::from_env()?;
    ///
    /// // If credentials are set, transactions work
    /// near.transfer("bob.testnet", "1 NEAR").await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `NEAR_ACCOUNT_ID` is set without `NEAR_PRIVATE_KEY` (or vice versa)
    /// - `NEAR_PRIVATE_KEY` contains an invalid key format
    /// - `NEAR_MAX_NONCE_RETRIES` is set but not a valid integer
    pub fn from_env() -> Result<Near, Error> {
        let network = std::env::var("NEAR_NETWORK").ok();
        let mut chain_id_override = std::env::var("NEAR_CHAIN_ID").ok();
        let account_id = std::env::var("NEAR_ACCOUNT_ID").ok();
        let private_key = std::env::var("NEAR_PRIVATE_KEY").ok();

        // Determine builder based on NEAR_NETWORK
        let mut builder = match network.as_deref() {
            Some("mainnet") => Near::mainnet(),
            Some("testnet") | None => Near::testnet(),
            Some(url) => {
                let chain_id = chain_id_override
                    .take()
                    .unwrap_or_else(|| "custom".to_string());
                Near::custom(url, chain_id)
            }
        };

        // Override chain_id if NEAR_CHAIN_ID is set (applies to mainnet/testnet presets)
        if let Some(id) = chain_id_override {
            builder = builder.chain_id(id);
        }

        // Configure signer if both account and key are provided
        match (account_id, private_key) {
            (Some(account), Some(key)) => {
                builder = builder.credentials(&key, account)?;
            }
            (Some(_), None) => {
                return Err(Error::Config(
                    "NEAR_ACCOUNT_ID is set but NEAR_PRIVATE_KEY is missing".into(),
                ));
            }
            (None, Some(_)) => {
                return Err(Error::Config(
                    "NEAR_PRIVATE_KEY is set but NEAR_ACCOUNT_ID is missing".into(),
                ));
            }
            (None, None) => {
                // Read-only client, no credentials
            }
        }

        // Configure max nonce retries if set
        if let Ok(retries) = std::env::var("NEAR_MAX_NONCE_RETRIES") {
            let retries: u32 = retries.parse().map_err(|_| {
                Error::Config(format!(
                    "NEAR_MAX_NONCE_RETRIES must be a non-negative integer, got: {retries}"
                ))
            })?;
            builder = builder.max_nonce_retries(retries);
        }

        Ok(builder.build())
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

        let signer = InMemorySigner::from_secret_key(account_id, secret_key)
            .expect("sandbox should provide valid account id");

        Near {
            rpc: Arc::new(RpcClient::new(network.rpc_url())),
            signer: Some(Arc::new(signer)),
            chain_id: ChainId::new(network.chain_id().unwrap_or("sandbox")),
            max_nonce_retries: 3,
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

    /// Get the signer's account ID.
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured. Use [`try_account_id`](Self::try_account_id)
    /// if you need to handle the no-signer case.
    pub fn account_id(&self) -> &AccountId {
        self.signer
            .as_ref()
            .expect("account_id() called on a Near client without a signer configured — use try_account_id() or configure a signer")
            .account_id()
    }

    /// Get the signer's account ID, if a signer is configured.
    pub fn try_account_id(&self) -> Option<&AccountId> {
        self.signer.as_ref().map(|s| s.account_id())
    }

    /// Get the signer's public key, if a signer is configured.
    ///
    /// This does not advance the rotation counter on [`RotatingSigner`](crate::RotatingSigner).
    pub fn public_key(&self) -> Option<PublicKey> {
        self.signer.as_ref().map(|s| s.public_key())
    }

    /// Get the signer, if one is configured.
    ///
    /// This is useful when you need to pass the signer to another system
    /// or construct clients manually.
    pub fn signer(&self) -> Option<Arc<dyn Signer>> {
        self.signer.clone()
    }

    /// Get the chain ID this client is connected to.
    pub fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    /// Set the number of nonce retries on `InvalidNonce` errors.
    ///
    /// `0` means no retries (send once), `1` means one retry, etc. Defaults to `3`.
    ///
    /// Useful when you need to adjust retries after construction,
    /// for example when using a client obtained from a sandbox.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # fn example(sandbox: Near) {
    /// let relayer = sandbox.max_nonce_retries(u32::MAX);
    /// # }
    /// ```
    pub fn max_nonce_retries(mut self, retries: u32) -> Near {
        self.max_nonce_retries = retries;
        self
    }

    /// Create a new client that shares this client's transport but uses a different signer.
    ///
    /// This is the recommended way to manage multiple accounts. The RPC connection
    /// is shared (via `Arc`), so there's no overhead from creating multiple clients.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # fn example() -> Result<(), Error> {
    /// // Set up a shared connection
    /// let near = Near::testnet().build();
    ///
    /// // Derive signing contexts for different accounts
    /// let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
    /// let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);
    ///
    /// // Both share the same RPC connection
    /// // alice.transfer("carol.testnet", NearToken::from_near(1)).await?;
    /// // bob.transfer("carol.testnet", NearToken::from_near(2)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_signer(&self, signer: impl Signer + 'static) -> Near {
        Near {
            rpc: self.rpc.clone(),
            signer: Some(Arc::new(signer)),
            chain_id: self.chain_id.clone(),
            max_nonce_retries: self.max_nonce_retries,
        }
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
    pub fn balance(&self, account_id: impl TryIntoAccountId) -> BalanceQuery {
        let account_id = account_id
            .try_into_account_id()
            .expect("invalid account ID");
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
    pub fn account(&self, account_id: impl TryIntoAccountId) -> AccountQuery {
        let account_id = account_id
            .try_into_account_id()
            .expect("invalid account ID");
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
    pub fn account_exists(&self, account_id: impl TryIntoAccountId) -> AccountExistsQuery {
        let account_id = account_id
            .try_into_account_id()
            .expect("invalid account ID");
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
    pub fn view<T>(&self, contract_id: impl TryIntoAccountId, method: &str) -> ViewCall<T> {
        let contract_id = contract_id
            .try_into_account_id()
            .expect("invalid account ID");
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
    pub fn access_keys(&self, account_id: impl TryIntoAccountId) -> AccessKeysQuery {
        let account_id = account_id
            .try_into_account_id()
            .expect("invalid account ID");
        AccessKeysQuery::new(self.rpc.clone(), account_id)
    }

    // ========================================================================
    // Validator / Epoch Queries
    // ========================================================================

    /// Get validator information for the latest epoch.
    ///
    /// Returns current validators, next epoch validators, current proposals,
    /// and kicked-out validators.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let info = near.validators().await?;
    /// println!("Current validators: {}", info.current_validators.len());
    /// println!("Epoch height: {}", info.epoch_height);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn validators(&self) -> Result<crate::types::EpochValidatorInfo, Error> {
        Ok(self.rpc.validators(None).await?)
    }

    /// Get validator information at a specific block height or hash.
    ///
    /// The RPC resolves the block to its epoch and returns that epoch's
    /// validators. Finality variants are treated as latest.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let info = near.validators_at(BlockReference::at_height(12345)).await?;
    /// println!("Epoch start height: {}", info.epoch_start_height);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn validators_at(
        &self,
        block: crate::types::BlockReference,
    ) -> Result<crate::types::EpochValidatorInfo, Error> {
        Ok(self.rpc.validators(Some(block)).await?)
    }

    /// Get ordered list of validators for the latest block.
    ///
    /// Returns validators ordered by their stake (highest first).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let ordered = near.validators_ordered().await?;
    /// for v in &ordered {
    ///     println!("{}: {}", v.account_id(), v.stake());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn validators_ordered(&self) -> Result<Vec<crate::types::ValidatorStakeView>, Error> {
        Ok(self.rpc.validators_ordered_latest().await?)
    }

    /// Get ordered list of validators at a specific block height or hash.
    ///
    /// Returns validators ordered by their stake (highest first).
    /// Finality variants are treated as latest.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let ordered = near.validators_ordered_at(BlockReference::at_height(12345)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn validators_ordered_at(
        &self,
        block: crate::types::BlockReference,
    ) -> Result<Vec<crate::types::ValidatorStakeView>, Error> {
        Ok(self.rpc.validators_ordered(block).await?)
    }

    // ========================================================================
    // Off-Chain Signing (NEP-413)
    // ========================================================================

    /// Sign a message for off-chain authentication (NEP-413).
    ///
    /// This enables users to prove account ownership without gas fees
    /// or blockchain transactions. Commonly used for:
    /// - Web3 authentication/login
    /// - Off-chain message signing
    /// - Proof of account ownership
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
    /// let signed = near.sign_message(nep413::SignMessageParams {
    ///     message: "Login to MyApp".to_string(),
    ///     recipient: "myapp.com".to_string(),
    ///     nonce: nep413::generate_nonce(),
    ///     callback_url: None,
    ///     state: None,
    /// }).await?;
    ///
    /// println!("Signed by: {}", signed.account_id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// @see <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>
    pub async fn sign_message(
        &self,
        params: crate::types::nep413::SignMessageParams,
    ) -> Result<crate::types::nep413::SignedMessage, Error> {
        let signer = self.signer.as_ref().ok_or(Error::NoSigner)?;
        let key = signer.key();
        key.sign_nep413(signer.account_id(), &params)
            .await
            .map_err(Error::Signing)
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
    /// near.transfer("bob.testnet", NearToken::from_near(1)).await?;
    ///
    /// // Transfer with wait for finality
    /// near.transfer("bob.testnet", NearToken::from_near(1000))
    ///     .wait_until(Final)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(
        &self,
        receiver: impl TryIntoAccountId,
        amount: impl IntoNearToken,
    ) -> TransactionBuilder {
        self.transaction(receiver).transfer(amount)
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
    pub fn call(&self, contract_id: impl TryIntoAccountId, method: &str) -> CallBuilder {
        self.transaction(contract_id).call(method)
    }

    /// Deploy WASM bytes to the signer's account.
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
    /// near.deploy(wasm_code).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured.
    pub fn deploy(&self, code: impl Into<Vec<u8>>) -> TransactionBuilder {
        let account_id = self.account_id().clone();
        self.transaction(account_id).deploy(code)
    }

    /// Deploy a contract from the global registry.
    ///
    /// Accepts either a `CryptoHash` (for immutable contracts identified by hash)
    /// or an account ID string/`AccountId` (for publisher-updatable contracts).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///         .credentials("ed25519:...", "alice.testnet")?
    ///     .build();
    ///
    /// // Deploy by publisher (updatable)
    /// near.deploy_from("publisher.near").await?;
    ///
    /// // Deploy by hash (immutable)
    /// near.deploy_from(code_hash).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured.
    pub fn deploy_from(&self, contract_ref: impl GlobalContractRef) -> TransactionBuilder {
        let account_id = self.account_id().clone();
        self.transaction(account_id).deploy_from(contract_ref)
    }

    /// Publish a contract to the global registry.
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
    ///
    /// // Publish updatable contract (identified by your account)
    /// near.publish(wasm_code.clone(), PublishMode::Updatable).await?;
    ///
    /// // Publish immutable contract (identified by its hash)
    /// near.publish(wasm_code, PublishMode::Immutable).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured.
    pub fn publish(&self, code: impl Into<Vec<u8>>, mode: PublishMode) -> TransactionBuilder {
        let account_id = self.account_id().clone();
        self.transaction(account_id).publish(code, mode)
    }

    /// Add a full access key to the signer's account.
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured.
    pub fn add_full_access_key(&self, public_key: PublicKey) -> TransactionBuilder {
        let account_id = self.account_id().clone();
        self.transaction(account_id).add_full_access_key(public_key)
    }

    /// Delete an access key from the signer's account.
    ///
    /// # Panics
    ///
    /// Panics if no signer is configured.
    pub fn delete_key(&self, public_key: PublicKey) -> TransactionBuilder {
        let account_id = self.account_id().clone();
        self.transaction(account_id).delete_key(public_key)
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
    pub fn transaction(&self, receiver_id: impl TryIntoAccountId) -> TransactionBuilder {
        let receiver_id = receiver_id
            .try_into_account_id()
            .expect("invalid account ID");
        TransactionBuilder::new(
            self.rpc.clone(),
            self.signer.clone(),
            receiver_id,
            self.max_nonce_retries,
        )
    }

    /// Create a NEP-616 deterministic state init transaction.
    ///
    /// The receiver_id is automatically derived from the state init parameters,
    /// so unlike [`transaction()`](Self::transaction), no receiver needs to be specified.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// let si = DeterministicAccountStateInit::by_hash(code_hash, Default::default());
    /// let outcome = near.state_init(si, NearToken::from_near(5))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the deposit amount string cannot be parsed.
    pub fn state_init(
        &self,
        state_init: crate::types::DeterministicAccountStateInit,
        deposit: impl IntoNearToken,
    ) -> TransactionBuilder {
        // Derive once and pass directly to avoid TransactionBuilder::state_init()
        // re-deriving the same account ID.
        let deposit = deposit
            .into_near_token()
            .expect("invalid deposit amount - use NearToken::from_str() for user input");
        let receiver_id = state_init.derive_account_id();
        self.transaction(receiver_id)
            .add_action(crate::types::Action::state_init(state_init, deposit))
    }

    /// Send a pre-signed transaction.
    ///
    /// Use this with transactions signed via `.sign()` for offline signing
    /// or inspection before sending.
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
    /// // Sign offline
    /// let signed = near.transfer("bob.testnet", NearToken::from_near(1))
    ///     .sign()
    ///     .await?;
    ///
    /// // Send later
    /// let outcome = near.send(&signed).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(
        &self,
        signed_tx: &crate::types::SignedTransaction,
    ) -> Result<crate::types::FinalExecutionOutcome, Error> {
        self.send_with_options(signed_tx, crate::types::ExecutedOptimistic)
            .await
    }

    /// Send a pre-signed transaction with a custom wait level.
    ///
    /// The return type depends on the wait level:
    /// - Executed levels ([`ExecutedOptimistic`](crate::types::ExecutedOptimistic),
    ///   [`Executed`](crate::types::Executed), [`Final`](crate::types::Final))
    ///   → [`FinalExecutionOutcome`](crate::types::FinalExecutionOutcome)
    /// - Non-executed levels ([`Submitted`](crate::types::Submitted),
    ///   [`Included`](crate::types::Included), [`IncludedFinal`](crate::types::IncludedFinal))
    ///   → [`SendTxResponse`](crate::types::SendTxResponse)
    pub async fn send_with_options<W: crate::types::WaitLevel>(
        &self,
        signed_tx: &crate::types::SignedTransaction,
        _level: W,
    ) -> Result<W::Response, Error> {
        let sender_id = &signed_tx.transaction.signer_id;
        let response = self.rpc.send_tx(signed_tx, W::status()).await?;
        W::convert(response, sender_id)
    }

    /// Get transaction status with full receipt details.
    ///
    /// Uses `EXPERIMENTAL_tx_status` under the hood. The return type depends
    /// on the wait level, just like [`send_with_options`](Self::send_with_options):
    ///
    /// - Executed levels ([`ExecutedOptimistic`](crate::types::ExecutedOptimistic),
    ///   [`Executed`](crate::types::Executed), [`Final`](crate::types::Final))
    ///   → [`FinalExecutionOutcome`](crate::types::FinalExecutionOutcome)
    ///   (with `receipts` populated)
    /// - Non-executed levels ([`Submitted`](crate::types::Submitted),
    ///   [`Included`](crate::types::Included), [`IncludedFinal`](crate::types::IncludedFinal))
    ///   → [`SendTxResponse`](crate::types::SendTxResponse)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: &Near, tx_hash: CryptoHash) -> Result<(), Error> {
    /// let outcome = near.tx_status(&tx_hash, "alice.testnet", Final).await?;
    /// println!("Gas used: {}", outcome.total_gas_used());
    /// println!("Receipts: {}", outcome.receipts.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn tx_status<W: crate::types::WaitLevel>(
        &self,
        tx_hash: &crate::types::CryptoHash,
        sender_id: impl crate::types::TryIntoAccountId,
        _level: W,
    ) -> Result<W::Response, Error> {
        let sender_id = sender_id.try_into_account_id()?;
        let response = self.rpc.tx_status(tx_hash, &sender_id, W::status()).await?;
        W::convert(response, &sender_id)
    }

    // ========================================================================
    // Convenience methods
    // ========================================================================

    /// Call a view function with arguments (convenience method).
    pub async fn view_with_args<T: DeserializeOwned + Send + 'static, A: serde::Serialize>(
        &self,
        contract_id: impl TryIntoAccountId,
        method: &str,
        args: &A,
    ) -> Result<T, Error> {
        let contract_id = contract_id.try_into_account_id()?;
        ViewCall::new(self.rpc.clone(), contract_id, method.to_string())
            .args(args)
            .await
    }

    /// Call a function with arguments (convenience method).
    pub async fn call_with_args<A: serde::Serialize>(
        &self,
        contract_id: impl TryIntoAccountId,
        method: &str,
        args: &A,
    ) -> Result<crate::types::FinalExecutionOutcome, Error> {
        self.call(contract_id, method).args(args).await
    }

    /// Call a function with full options (convenience method).
    pub async fn call_with_options<A: serde::Serialize>(
        &self,
        contract_id: impl TryIntoAccountId,
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
    pub fn contract<T: crate::Contract>(&self, contract_id: impl TryIntoAccountId) -> T::Client {
        let contract_id = contract_id
            .try_into_account_id()
            .expect("invalid account ID");
        T::Client::new(self.clone(), contract_id)
    }

    // ========================================================================
    // Token Helpers
    // ========================================================================

    /// Get a fungible token client for a NEP-141 contract.
    ///
    /// Accepts either a string/`AccountId` for raw addresses, or a [`KnownToken`]
    /// constant (like [`tokens::USDC`]) which auto-resolves based on the network.
    ///
    /// [`KnownToken`]: crate::tokens::KnownToken
    /// [`tokens::USDC`]: crate::tokens::USDC
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::mainnet().build();
    ///
    /// // Use a known token - auto-resolves based on network
    /// let usdc = near.ft(tokens::USDC)?;
    ///
    /// // Or use a raw address
    /// let custom = near.ft("custom-token.near")?;
    ///
    /// // Get metadata
    /// let meta = usdc.metadata().await?;
    /// println!("{} ({})", meta.name, meta.symbol);
    ///
    /// // Get balance - returns FtAmount for nice formatting
    /// let balance = usdc.balance_of("alice.near").await?;
    /// println!("Balance: {}", balance);  // e.g., "1.5 USDC"
    /// # Ok(())
    /// # }
    /// ```
    pub fn ft(
        &self,
        contract: impl crate::tokens::IntoContractId,
    ) -> Result<crate::tokens::FungibleToken, Error> {
        let contract_id = contract.into_contract_id(&self.chain_id)?;
        Ok(crate::tokens::FungibleToken::new(
            self.rpc.clone(),
            self.signer.clone(),
            contract_id,
            self.max_nonce_retries,
        ))
    }

    /// Get a non-fungible token client for a NEP-171 contract.
    ///
    /// Accepts either a string/`AccountId` for raw addresses, or a contract
    /// identifier that implements [`IntoContractId`].
    ///
    /// [`IntoContractId`]: crate::tokens::IntoContractId
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let nft = near.nft("nft-contract.near")?;
    ///
    /// // Get a specific token
    /// if let Some(token) = nft.token("token-123").await? {
    ///     println!("Owner: {}", token.owner_id);
    /// }
    ///
    /// // List tokens for an owner
    /// let tokens = nft.tokens_for_owner("alice.near", None, Some(10)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn nft(
        &self,
        contract: impl crate::tokens::IntoContractId,
    ) -> Result<crate::tokens::NonFungibleToken, Error> {
        let contract_id = contract.into_contract_id(&self.chain_id)?;
        Ok(crate::tokens::NonFungibleToken::new(
            self.rpc.clone(),
            self.signer.clone(),
            contract_id,
            self.max_nonce_retries,
        ))
    }
}

impl std::fmt::Debug for Near {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Near")
            .field("rpc", &self.rpc)
            .field("account_id", &self.try_account_id())
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
    chain_id: ChainId,
    max_nonce_retries: u32,
}

impl NearBuilder {
    /// Create a new builder with the given RPC URL.
    fn new(rpc_url: impl Into<String>, chain_id: ChainId) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            signer: None,
            retry_config: RetryConfig::default(),
            chain_id,
            max_nonce_retries: 3,
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
        account_id: impl TryIntoAccountId,
    ) -> Result<Self, Error> {
        let signer = InMemorySigner::new(account_id, private_key)?;
        self.signer = Some(Arc::new(signer));
        Ok(self)
    }

    /// Set the chain ID.
    ///
    /// This is useful for custom networks where the default chain ID
    /// (e.g., `"custom"`) should be overridden.
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = ChainId::new(chain_id);
        self
    }

    /// Set the retry configuration.
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Set the maximum number of transaction send attempts on `InvalidNonce` errors.
    ///
    /// When a transaction fails with `InvalidNonce`, the client automatically
    /// retries with the corrected nonce from the error response.
    ///
    /// `0` means no retries (send once), `1` means one retry, etc. Defaults to `3`.
    /// For high-contention relayer scenarios, consider setting
    /// this higher (e.g., `u32::MAX`) and wrapping sends in `tokio::timeout`.
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.max_nonce_retries = retries;
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
            chain_id: self.chain_id,
            max_nonce_retries: self.max_nonce_retries,
        }
    }
}

impl From<NearBuilder> for Near {
    fn from(builder: NearBuilder) -> Self {
        builder.build()
    }
}

/// Default sandbox root account ID.
pub const SANDBOX_ROOT_ACCOUNT: &str = "sandbox";

/// Default sandbox root secret key.
///
/// Deterministic key generated via `near-sandbox init --test-seed sandbox`.
pub const SANDBOX_ROOT_SECRET_KEY: &str = "ed25519:3JoAjwLppjgvxkk6kNsu5wQj3FfUJnpBKWieC73hVTpBeA6FZiCc5tfyZL3a3tHeQJegQe4qGSv8FLsYp7TYd1r6";

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Near client tests
    // ========================================================================

    #[test]
    fn test_near_mainnet_builder() {
        let near = Near::mainnet().build();
        assert!(near.rpc_url().contains("fastnear") || near.rpc_url().contains("near"));
        assert!(near.try_account_id().is_none()); // No signer configured
    }

    #[test]
    fn test_near_testnet_builder() {
        let near = Near::testnet().build();
        assert!(near.rpc_url().contains("fastnear") || near.rpc_url().contains("test"));
        assert!(near.try_account_id().is_none());
    }

    #[test]
    fn test_near_custom_builder() {
        let near = Near::custom("https://custom-rpc.example.com", "mainnet").build();
        assert_eq!(near.rpc_url(), "https://custom-rpc.example.com");
        assert!(near.chain_id().is_mainnet());
    }

    #[test]
    fn test_near_custom_builder_with_chain_id_type() {
        let near = Near::custom("https://rpc.example.com", ChainId::testnet()).build();
        assert!(near.chain_id().is_testnet());
    }

    #[test]
    fn test_near_with_credentials() {
        let near = Near::testnet()
            .credentials(
                "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
                "alice.testnet",
            )
            .unwrap()
            .build();

        assert_eq!(near.account_id().as_str(), "alice.testnet");
    }

    #[test]
    fn test_near_with_signer() {
        let signer = InMemorySigner::new(
            "bob.testnet",
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
        ).unwrap();

        let near = Near::testnet().signer(signer).build();

        assert_eq!(near.account_id().as_str(), "bob.testnet");
    }

    #[test]
    fn test_near_debug() {
        let near = Near::testnet().build();
        let debug = format!("{:?}", near);
        assert!(debug.contains("Near"));
        assert!(debug.contains("rpc"));
    }

    #[test]
    fn test_near_rpc_accessor() {
        let near = Near::testnet().build();
        let rpc = near.rpc();
        assert!(!rpc.url().is_empty());
    }

    // ========================================================================
    // NearBuilder tests
    // ========================================================================

    #[test]
    fn test_near_builder_new() {
        let builder = NearBuilder::new("https://example.com", ChainId::new("custom"));
        let near = builder.build();
        assert_eq!(near.rpc_url(), "https://example.com");
    }

    #[test]
    fn test_near_builder_retry_config() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 200,
            max_delay_ms: 10000,
        };
        let near = Near::testnet().retry_config(config).build();
        // Can't directly test retry config, but we can verify it builds
        assert!(!near.rpc_url().is_empty());
    }

    #[test]
    fn test_near_builder_from_trait() {
        let builder = Near::testnet();
        let near: Near = builder.into();
        assert!(!near.rpc_url().is_empty());
    }

    #[test]
    fn test_near_builder_credentials_invalid_key() {
        let result = Near::testnet().credentials("invalid-key", "alice.testnet");
        assert!(result.is_err());
    }

    #[test]
    fn test_near_builder_credentials_invalid_account() {
        // Empty account ID is now rejected by the upstream AccountId parser.
        let result = Near::testnet().credentials(
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
            "",
        );
        assert!(result.is_err());
    }

    // ========================================================================
    // SandboxNetwork trait tests
    // ========================================================================

    struct MockSandbox {
        rpc_url: String,
        root_account: String,
        root_key: String,
    }

    impl SandboxNetwork for MockSandbox {
        fn rpc_url(&self) -> &str {
            &self.rpc_url
        }

        fn root_account_id(&self) -> &str {
            &self.root_account
        }

        fn root_secret_key(&self) -> &str {
            &self.root_key
        }
    }

    #[test]
    fn test_sandbox_network_trait() {
        let mock = MockSandbox {
            rpc_url: "http://127.0.0.1:3030".to_string(),
            root_account: "sandbox".to_string(),
            root_key: SANDBOX_ROOT_SECRET_KEY.to_string(),
        };

        let near = Near::sandbox(&mock);
        assert_eq!(near.rpc_url(), "http://127.0.0.1:3030");
        assert_eq!(near.account_id().as_str(), "sandbox");
    }

    // ========================================================================
    // Constant tests
    // ========================================================================

    #[test]
    fn test_sandbox_constants() {
        assert_eq!(SANDBOX_ROOT_ACCOUNT, "sandbox");
        assert!(SANDBOX_ROOT_SECRET_KEY.starts_with("ed25519:"));
    }

    // ========================================================================
    // Clone tests
    // ========================================================================

    #[test]
    fn test_near_clone() {
        let near1 = Near::testnet().build();
        let near2 = near1.clone();
        assert_eq!(near1.rpc_url(), near2.rpc_url());
    }

    #[test]
    fn test_near_with_signer_derived() {
        let near = Near::testnet().build();
        assert!(near.try_account_id().is_none());

        let signer = InMemorySigner::new(
            "alice.testnet",
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
        ).unwrap();

        let alice = near.with_signer(signer);
        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(alice.rpc_url(), near.rpc_url()); // Same transport
        assert!(near.try_account_id().is_none()); // Original unchanged
    }

    #[test]
    fn test_near_with_signer_multiple_accounts() {
        let near = Near::testnet().build();

        let alice = near.with_signer(InMemorySigner::new(
            "alice.testnet",
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
        ).unwrap());

        let bob = near.with_signer(InMemorySigner::new(
            "bob.testnet",
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
        ).unwrap());

        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(bob.account_id().as_str(), "bob.testnet");
        assert_eq!(alice.rpc_url(), bob.rpc_url()); // Shared transport
    }

    #[test]
    fn test_near_max_nonce_retries() {
        let near = Near::testnet().build();
        assert_eq!(near.max_nonce_retries, 3);

        let near = near.max_nonce_retries(10);
        assert_eq!(near.max_nonce_retries, 10);

        // 0 means no retries (send once)
        let near = near.max_nonce_retries(0);
        assert_eq!(near.max_nonce_retries, 0);
    }

    // ========================================================================
    // from_env tests
    // ========================================================================

    // NOTE: Environment variable tests are consolidated into a single test
    // because they modify global state and would race with each other if
    // run in parallel. Each scenario is tested sequentially within this test.
    #[test]
    fn test_from_env_scenarios() {
        // Helper to clean up env vars
        fn clear_env() {
            // SAFETY: This is a test and we control the execution
            unsafe {
                std::env::remove_var("NEAR_CHAIN_ID");
                std::env::remove_var("NEAR_NETWORK");
                std::env::remove_var("NEAR_ACCOUNT_ID");
                std::env::remove_var("NEAR_PRIVATE_KEY");
                std::env::remove_var("NEAR_MAX_NONCE_RETRIES");
            }
        }

        // Scenario 1: No vars - defaults to testnet, read-only
        clear_env();
        {
            let near = Near::from_env().unwrap();
            assert!(
                near.rpc_url().contains("test") || near.rpc_url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.rpc_url()
            );
            assert!(near.try_account_id().is_none());
        }

        // Scenario 2: Mainnet network
        clear_env();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "mainnet");
        }
        {
            let near = Near::from_env().unwrap();
            assert!(
                near.rpc_url().contains("mainnet") || near.rpc_url().contains("fastnear"),
                "Expected mainnet URL, got: {}",
                near.rpc_url()
            );
            assert!(near.try_account_id().is_none());
        }

        // Scenario 3: Custom URL
        clear_env();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "https://custom-rpc.example.com");
        }
        {
            let near = Near::from_env().unwrap();
            assert_eq!(near.rpc_url(), "https://custom-rpc.example.com");
        }

        // Scenario 4: Full credentials
        clear_env();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "testnet");
            std::env::set_var("NEAR_ACCOUNT_ID", "alice.testnet");
            std::env::set_var(
                "NEAR_PRIVATE_KEY",
                "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
            );
        }
        {
            let near = Near::from_env().unwrap();
            assert_eq!(near.account_id().as_str(), "alice.testnet");
        }

        // Scenario 5: Account without key - should error
        clear_env();
        unsafe {
            std::env::set_var("NEAR_ACCOUNT_ID", "alice.testnet");
        }
        {
            let result = Near::from_env();
            assert!(
                result.is_err(),
                "Expected error when account set without key"
            );
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("NEAR_PRIVATE_KEY"),
                "Error should mention NEAR_PRIVATE_KEY: {}",
                err
            );
        }

        // Scenario 6: Key without account - should error
        clear_env();
        unsafe {
            std::env::set_var(
                "NEAR_PRIVATE_KEY",
                "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
            );
        }
        {
            let result = Near::from_env();
            assert!(
                result.is_err(),
                "Expected error when key set without account"
            );
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("NEAR_ACCOUNT_ID"),
                "Error should mention NEAR_ACCOUNT_ID: {}",
                err
            );
        }

        // Scenario 7: Custom max_nonce_retries
        clear_env();
        unsafe {
            std::env::set_var("NEAR_MAX_NONCE_RETRIES", "10");
        }
        {
            let near = Near::from_env().unwrap();
            assert_eq!(near.max_nonce_retries, 10);
        }

        // Scenario 8: Invalid max_nonce_retries (not a number)
        clear_env();
        unsafe {
            std::env::set_var("NEAR_MAX_NONCE_RETRIES", "abc");
        }
        {
            let result = Near::from_env();
            assert!(
                result.is_err(),
                "Expected error for non-numeric NEAR_MAX_NONCE_RETRIES"
            );
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("NEAR_MAX_NONCE_RETRIES"),
                "Error should mention NEAR_MAX_NONCE_RETRIES: {}",
                err
            );
        }

        // Scenario 9: max_nonce_retries zero is valid (means no retries)
        clear_env();
        unsafe {
            std::env::set_var("NEAR_MAX_NONCE_RETRIES", "0");
        }
        {
            let near = Near::from_env().expect("0 retries should be valid");
            assert_eq!(near.max_nonce_retries, 0);
        }

        // Scenario 10: NEAR_CHAIN_ID overrides chain_id for custom network
        clear_env();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "https://rpc.pinet.near.org");
            std::env::set_var("NEAR_CHAIN_ID", "pinet");
        }
        {
            let near = Near::from_env().unwrap();
            assert_eq!(near.rpc_url(), "https://rpc.pinet.near.org");
            assert_eq!(near.chain_id().as_str(), "pinet");
        }

        // Scenario 11: NEAR_CHAIN_ID without NEAR_NETWORK defaults to testnet RPC
        clear_env();
        unsafe {
            std::env::set_var("NEAR_CHAIN_ID", "my-chain");
        }
        {
            let near = Near::from_env().unwrap();
            assert!(
                near.rpc_url().contains("test") || near.rpc_url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.rpc_url()
            );
            assert_eq!(near.chain_id().as_str(), "my-chain");
        }

        // Final cleanup
        clear_env();
    }
}
