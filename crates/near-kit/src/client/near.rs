//! The main Near client.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::contract::ContractClient;
use crate::error::Error;
use crate::types::{AccountId, Gas, IntoNearToken, NearToken, Network, PublicKey, SecretKey};

use super::query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
use super::rpc::{MAINNET, RetryConfig, RpcClient, TESTNET};
use super::signer::{InMemorySigner, Signer};
use super::transaction::{CallBuilder, TransactionBuilder};
use crate::types::TxExecutionStatus;

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
    network: Network,
}

impl Near {
    /// Create a builder for mainnet.
    pub fn mainnet() -> NearBuilder {
        NearBuilder::new(MAINNET.rpc_url, Network::Mainnet)
    }

    /// Create a builder for testnet.
    pub fn testnet() -> NearBuilder {
        NearBuilder::new(TESTNET.rpc_url, Network::Testnet)
    }

    /// Create a builder with a custom RPC URL.
    pub fn custom(rpc_url: impl Into<String>) -> NearBuilder {
        NearBuilder::new(rpc_url, Network::Custom)
    }

    /// Create a configured client from environment variables.
    ///
    /// Reads the following environment variables:
    /// - `NEAR_NETWORK` (optional): `"mainnet"`, `"testnet"`, or a custom RPC URL.
    ///   Defaults to `"testnet"` if not set.
    /// - `NEAR_ACCOUNT_ID` (optional): Account ID for signing transactions.
    /// - `NEAR_PRIVATE_KEY` (optional): Private key for signing (e.g., `"ed25519:..."`).
    ///
    /// If `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` are both set, the client will
    /// be configured with signing capability. Otherwise, it will be read-only.
    ///
    /// # Example
    ///
    /// ```bash
    /// # Environment variables
    /// export NEAR_NETWORK=testnet
    /// export NEAR_ACCOUNT_ID=alice.testnet
    /// export NEAR_PRIVATE_KEY=ed25519:...
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
    pub fn from_env() -> Result<Near, Error> {
        let network = std::env::var("NEAR_NETWORK").ok();
        let account_id = std::env::var("NEAR_ACCOUNT_ID").ok();
        let private_key = std::env::var("NEAR_PRIVATE_KEY").ok();

        // Determine builder based on network
        let mut builder = match network.as_deref() {
            Some("mainnet") => Near::mainnet(),
            Some("testnet") | None => Near::testnet(),
            Some(url) => Near::custom(url),
        };

        // Configure signer if both account and key are provided
        match (account_id, private_key) {
            (Some(account), Some(key)) => {
                builder = builder.credentials(&key, &account)?;
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

        let signer = InMemorySigner::from_secret_key(account_id, secret_key);

        Near {
            rpc: Arc::new(RpcClient::new(network.rpc_url())),
            signer: Some(Arc::new(signer)),
            network: Network::Sandbox,
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

    /// Get the network this client is connected to.
    pub fn network(&self) -> Network {
        self.network
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
        let account_id = AccountId::parse_lenient(account_id);
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
        let account_id = AccountId::parse_lenient(account_id);
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
        let account_id = AccountId::parse_lenient(account_id);
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
    pub fn view<T>(&self, contract_id: impl AsRef<str>, method: &str) -> ViewCall<T> {
        let contract_id = AccountId::parse_lenient(contract_id);
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
        let account_id = AccountId::parse_lenient(account_id);
        AccessKeysQuery::new(self.rpc.clone(), account_id)
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
    /// near.transfer("bob.testnet", NearToken::near(1)).await?;
    ///
    /// // Transfer with wait for finality
    /// near.transfer("bob.testnet", NearToken::near(1000))
    ///     .wait_until(TxExecutionStatus::Final)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(
        &self,
        receiver: impl AsRef<str>,
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
    pub fn call(&self, contract_id: impl AsRef<str>, method: &str) -> CallBuilder {
        self.transaction(contract_id).call(method)
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
    pub fn deploy(
        &self,
        account_id: impl AsRef<str>,
        code: impl Into<Vec<u8>>,
    ) -> TransactionBuilder {
        self.transaction(account_id).deploy(code)
    }

    /// Add a full access key to an account.
    pub fn add_full_access_key(
        &self,
        account_id: impl AsRef<str>,
        public_key: PublicKey,
    ) -> TransactionBuilder {
        self.transaction(account_id).add_full_access_key(public_key)
    }

    /// Delete an access key from an account.
    pub fn delete_key(
        &self,
        account_id: impl AsRef<str>,
        public_key: PublicKey,
    ) -> TransactionBuilder {
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
    pub fn transaction(&self, receiver_id: impl AsRef<str>) -> TransactionBuilder {
        let receiver_id = AccountId::parse_lenient(receiver_id);
        TransactionBuilder::new(self.rpc.clone(), self.signer.clone(), receiver_id)
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
    /// let signed = near.transfer("bob.testnet", NearToken::near(1))
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
        self.send_with_options(signed_tx, TxExecutionStatus::ExecutedOptimistic)
            .await
    }

    /// Send a pre-signed transaction with custom wait options.
    pub async fn send_with_options(
        &self,
        signed_tx: &crate::types::SignedTransaction,
        wait_until: TxExecutionStatus,
    ) -> Result<crate::types::FinalExecutionOutcome, Error> {
        let outcome = self.rpc.send_tx(signed_tx, wait_until).await?;

        if outcome.is_failure() {
            return Err(Error::TransactionFailed(
                outcome.failure_message().unwrap_or_default(),
            ));
        }

        Ok(outcome)
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
        let contract_id = AccountId::parse_lenient(contract_id);
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
        let contract_id = AccountId::parse_lenient(contract_id);
        T::Client::new(self, contract_id)
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
        let contract_id = contract.into_contract_id(self.network)?;
        Ok(crate::tokens::FungibleToken::new(
            self.rpc.clone(),
            self.signer.clone(),
            contract_id,
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
        let contract_id = contract.into_contract_id(self.network)?;
        Ok(crate::tokens::NonFungibleToken::new(
            self.rpc.clone(),
            self.signer.clone(),
            contract_id,
        ))
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
    network: Network,
}

impl NearBuilder {
    /// Create a new builder with the given RPC URL.
    fn new(rpc_url: impl Into<String>, network: Network) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            signer: None,
            retry_config: RetryConfig::default(),
            network,
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
            network: self.network,
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
pub const SANDBOX_ROOT_PRIVATE_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

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
        assert!(near.account_id().is_none()); // No signer configured
    }

    #[test]
    fn test_near_testnet_builder() {
        let near = Near::testnet().build();
        assert!(near.rpc_url().contains("fastnear") || near.rpc_url().contains("test"));
        assert!(near.account_id().is_none());
    }

    #[test]
    fn test_near_custom_builder() {
        let near = Near::custom("https://custom-rpc.example.com").build();
        assert_eq!(near.rpc_url(), "https://custom-rpc.example.com");
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

        assert!(near.account_id().is_some());
        assert_eq!(near.account_id().unwrap().as_str(), "alice.testnet");
    }

    #[test]
    fn test_near_with_signer() {
        let signer = InMemorySigner::new(
            "bob.testnet",
            "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB",
        ).unwrap();

        let near = Near::testnet().signer(signer).build();

        assert!(near.account_id().is_some());
        assert_eq!(near.account_id().unwrap().as_str(), "bob.testnet");
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
        let builder = NearBuilder::new("https://example.com", Network::Custom);
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
        // Empty account ID is invalid
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
            root_key: SANDBOX_ROOT_PRIVATE_KEY.to_string(),
        };

        let near = Near::sandbox(&mock);
        assert_eq!(near.rpc_url(), "http://127.0.0.1:3030");
        assert!(near.account_id().is_some());
        assert_eq!(near.account_id().unwrap().as_str(), "sandbox");
    }

    // ========================================================================
    // Constant tests
    // ========================================================================

    #[test]
    fn test_sandbox_constants() {
        assert_eq!(SANDBOX_ROOT_ACCOUNT, "sandbox");
        assert!(SANDBOX_ROOT_PRIVATE_KEY.starts_with("ed25519:"));
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
                std::env::remove_var("NEAR_NETWORK");
                std::env::remove_var("NEAR_ACCOUNT_ID");
                std::env::remove_var("NEAR_PRIVATE_KEY");
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
            assert!(near.account_id().is_none());
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
            assert!(near.account_id().is_none());
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
            assert!(near.account_id().is_some());
            assert_eq!(near.account_id().unwrap().as_str(), "alice.testnet");
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

        // Final cleanup
        clear_env();
    }
}
