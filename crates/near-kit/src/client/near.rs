//! The main Near client.

use std::future::Future;
use std::sync::Arc;

use crate::error::{Error, RpcError};
use crate::platform::MaybeSend;
use crate::types::{
    AccountId, BlockReference, ChainId, TryIntoAccountId, TryIntoGlobalContractId,
    ViewFunctionResult,
};

use super::query::{
    AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ContractCodeQuery,
    GlobalContractQuery, TransactionStatusQuery, ViewCall,
};
use super::rpc::{MAINNET, RetryConfig, RpcClient, TESTNET};
use super::signing::SigningNear;
use super::transaction::SignedTransactionSend;
use super::transport::{self, RpcTransport};
use crate::types::Signer;

/// Nonce retries a new client allows unless configured otherwise.
const DEFAULT_MAX_NONCE_RETRIES: u32 = 3;

/// The read-only client for NEAR Protocol.
///
/// `Near` holds the RPC connection and network configuration. It reads chain
/// state and submits pre-signed transactions, but it never holds a signer:
/// attach one with [`with_signer`](Near::with_signer) to get a
/// [`SigningNear`] that can send transactions. Every `SigningNear` derived
/// from one `Near` shares its `Arc<RpcClient>`.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
/// use near_kit::signer::InMemorySigner;
/// use near_kit::protocol::Action;
///
/// #[tokio::main]
/// async fn main() -> Result<(), near_kit::Error> {
///     let near = Near::testnet().build();
///     let balance = near.balance("alice.testnet").await?;
///     println!("Balance: {}", balance);
///
///     let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
///     alice
///         .transaction(account!("bob.testnet"))
///         .add_action(Action::transfer(NearToken::from_near(1)))
///         .await?;
///
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Near {
    pub(super) rpc: Arc<RpcClient>,
    chain_id: ChainId,
    pub(super) max_nonce_retries: u32,
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

    /// Wrap an existing [`RpcClient`] in a `Near` client.
    ///
    /// The client keeps the RPC client's URL, transport and retry
    /// configuration. Use this when a component is handed an `RpcClient` but
    /// wants the high-level API on top of it, such as typed
    /// `#[near_kit::contract]` views. Add a signer with
    /// [`with_signer`](Self::with_signer).
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit::Near;
    /// use near_kit::rpc::RpcClient;
    ///
    /// let rpc = RpcClient::new("https://rpc.testnet.near.org");
    /// let near = Near::from_rpc(rpc, "testnet");
    /// assert_eq!(near.chain_id().as_str(), "testnet");
    /// ```
    pub fn from_rpc(rpc: RpcClient, chain_id: impl Into<ChainId>) -> Near {
        Near {
            rpc: Arc::new(rpc),
            chain_id: chain_id.into(),
            max_nonce_retries: DEFAULT_MAX_NONCE_RETRIES,
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

    /// Get the chain ID this client is connected to.
    pub fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    /// Set the number of nonce retries on `InvalidNonce` errors.
    ///
    /// `0` means no retries (send once), `1` means one retry, etc. Defaults to `3`.
    ///
    /// Useful when you need to adjust retries after construction,
    /// for example when using a client obtained from a sandbox. A
    /// [`SigningNear`] takes the setting of the `Near` it was created from.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # fn example(sandbox: SigningNear) {
    /// let relayer = sandbox
    ///     .near()
    ///     .clone()
    ///     .max_nonce_retries(u32::MAX)
    ///     .with_signer(sandbox.signer().clone());
    /// # }
    /// ```
    pub fn max_nonce_retries(mut self, retries: u32) -> Near {
        self.max_nonce_retries = retries;
        self
    }

    /// Attach a signer, giving a [`SigningNear`] that sends transactions as
    /// the signer's account.
    ///
    /// The RPC connection is shared (via `Arc`), so deriving one signing
    /// client per account costs nothing. The result waits for
    /// [`Final`](crate::transaction::Final), the level reads default to; pick
    /// another level once with
    /// [`SigningNear::wait_until`](SigningNear::wait_until).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::signer::InMemorySigner;
    /// # fn example() -> Result<(), Error> {
    /// let near = Near::testnet().build();
    ///
    /// let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
    /// let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);
    /// assert_eq!(alice.account_id().as_str(), "alice.testnet");
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_signer(&self, signer: impl Signer + 'static) -> SigningNear {
        SigningNear::new(self.clone(), Arc::new(signer))
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
    /// # use near_kit::rpc::Finality;
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
        AccessKeysQuery::new(self.rpc.clone(), account_id)
    }

    /// Get the WASM code deployed on an account.
    ///
    /// Returns the code bytes plus their SHA-256 hash. Use `.exists()` to
    /// check for a deployed contract without caring about the code itself.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// let contract = near.contract_code("app.near").await?;
    /// println!("code hash: {}, {} bytes", contract.hash, contract.code.len());
    ///
    /// if near.contract_code("app.near").exists().await? {
    ///     println!("Contract deployed!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn contract_code(&self, account_id: impl TryIntoAccountId) -> ContractCodeQuery {
        ContractCodeQuery::new(self.rpc.clone(), account_id)
    }

    /// Get a global contract's code and hash.
    ///
    /// Accepts the same identifiers as `Action::deploy_from_account` and
    /// `Action::deploy_from_hash`: a publisher
    /// account ID (updatable contracts, where `hash` reflects the currently
    /// published version) or a code hash (immutable contracts). Use
    /// `.exists()` to check whether anything is published under the
    /// identifier.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// // By publisher account (updatable)
    /// let contract = near.global_contract("publisher.near").await?;
    /// println!("current code hash: {}", contract.hash);
    ///
    /// // By code hash (immutable)
    /// let contract = near.global_contract(code_hash).await?;
    ///
    /// // Deployment check
    /// if near.global_contract("publisher.near").exists().await? {
    ///     println!("Global contract published!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn global_contract(&self, id: impl TryIntoGlobalContractId) -> GlobalContractQuery {
        GlobalContractQuery::new(self.rpc.clone(), id)
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

    /// Send a pre-signed transaction.
    ///
    /// Use this with transactions signed via
    /// [`SigningNear::sign`](SigningNear::sign) (or offline with
    /// [`Transaction::sign`](crate::protocol::Transaction::sign)) to inspect
    /// them or send them later. There is no nonce retry here: the signature
    /// fixes the nonce.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # use near_kit::rpc::FinalExecutionOutcome;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let signed = alice
    ///     .sign(account!("bob.testnet"), vec![Action::transfer(NearToken::from_near(1))])
    ///     .await?;
    ///
    /// // Send later. The default waits for optimistic execution.
    /// let outcome: FinalExecutionOutcome = alice.near().send_signed(&signed).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn send_signed<'tx>(
        &self,
        signed_tx: &'tx crate::types::SignedTransaction,
    ) -> SignedTransactionSend<'tx> {
        SignedTransactionSend::new(self.rpc.clone(), signed_tx)
    }

    /// Query transaction status and receipt progress.
    ///
    /// Uses `EXPERIMENTAL_tx_status` under the hood and returns an awaitable
    /// [`TransactionStatusQuery`]. Awaiting it directly uses
    /// [`Submitted`](crate::transaction::Submitted), so it returns the node's current
    /// progress without waiting for a new milestone. Chain
    /// [`.wait_until::<W>()`](TransactionStatusQuery::wait_until) to wait for a
    /// specific level.
    ///
    /// - Executed levels ([`ExecutedOptimistic`](crate::transaction::ExecutedOptimistic),
    ///   [`Executed`](crate::transaction::Executed), [`Final`](crate::transaction::Final))
    ///   → [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome)
    ///   (with `receipts` populated)
    /// - Non-executed levels ([`Submitted`](crate::transaction::Submitted),
    ///   [`Included`](crate::transaction::Included), [`IncludedFinal`](crate::transaction::IncludedFinal))
    ///   → [`SendTxResponse`](crate::rpc::SendTxResponse), whose
    ///   [`outcome`](crate::rpc::SendTxResponse::outcome) carries the *partial*
    ///   execution outcome (`receipts_outcome`/`receipts`) as soon as the node
    ///   has it. Unlike `send_tx`, `EXPERIMENTAL_tx_status` returns receipt data
    ///   even at these early levels, so a frontend can poll here to drive a
    ///   progressive UI without blocking for finality.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::transaction::Final;
    /// # async fn example(near: &Near, tx_hash: CryptoHash) -> Result<(), Error> {
    /// let outcome = near
    ///     .tx_status(&tx_hash, "alice.testnet")
    ///     .wait_until::<Final>()
    ///     .await?;
    /// println!("Gas used: {}", outcome.total_gas_used());
    /// println!("Receipts: {}", outcome.receipts.len());
    ///
    /// // Poll current progress without blocking:
    /// let status = near.tx_status(&tx_hash, "alice.testnet").await?;
    /// if let Some(partial) = &status.outcome {
    ///     println!("Receipts so far: {}", partial.receipts_outcome.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn tx_status(
        &self,
        tx_hash: &crate::types::CryptoHash,
        sender_id: impl crate::types::TryIntoAccountId,
    ) -> TransactionStatusQuery {
        TransactionStatusQuery::new(self.rpc.clone(), *tx_hash, sender_id)
    }

    // ========================================================================
    // Typed Contract Interfaces
    // ========================================================================

    /// Create a typed contract client backed by this `Near`.
    ///
    /// `T` is the marker type `#[near_kit::contract]` generates for a trait;
    /// the result is its generated `<Name>Client<Near>`. View methods return
    /// a [`View`](crate::transaction::View) and `#[call]` methods a
    /// [`Call`](crate::transaction::Call); both are `.await`ed directly.
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
    ///     fn add(&mut self, args: AddArgs);
    /// }
    ///
    /// #[derive(Serialize)]
    /// pub struct AddArgs {
    ///     pub value: u64,
    /// }
    ///
    /// async fn example(near: &Near) -> Result<(), near_kit::Error> {
    ///     let counter = near.contract::<Counter>(account!("counter.testnet"));
    ///     let count = counter.get_count().await?;
    ///     counter.add(AddArgs { value: 5 }).await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// Takes an already-validated account ID (an [`AccountId`], `&AccountId`,
    /// or `&AccountIdRef`), so creating the client cannot fail. Use
    /// [`account!`](crate::account) for a literal.
    pub fn contract<T: crate::Contract>(
        &self,
        contract_id: impl Into<AccountId>,
    ) -> T::Client<Near> {
        T::client(self.clone(), contract_id.into())
    }
}

impl crate::Viewer for Near {
    type ViewError = RpcError;

    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, RpcError>> + MaybeSend {
        self.rpc.view_function(contract_id, method, args, block)
    }
}

impl std::fmt::Debug for Near {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Near")
            .field("rpc", &self.rpc)
            .field("chain_id", &self.chain_id)
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
/// let near = Near::testnet()
///     .max_nonce_retries(10)
///     .build();
/// ```
pub struct NearBuilder {
    rpc_url: String,
    /// `None` until [`NearBuilder::build`] — resolved lazily so that injecting
    /// a transport never constructs the target's default. On WASI without
    /// `wasi-http`, that default reports a non-retryable transport error.
    transport: Option<Arc<dyn RpcTransport>>,
    retry_config: RetryConfig,
    chain_id: ChainId,
    max_nonce_retries: u32,
}

impl NearBuilder {
    /// Create a new builder with the given RPC URL.
    fn new(rpc_url: impl Into<String>, chain_id: ChainId) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            transport: None,
            retry_config: RetryConfig::default(),
            chain_id,
            max_nonce_retries: DEFAULT_MAX_NONCE_RETRIES,
        }
    }

    /// Set the chain ID.
    ///
    /// This is useful for custom networks where the default chain ID
    /// (e.g., `"custom"`) should be overridden.
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = ChainId::new(chain_id);
        self
    }

    /// Set the retry configuration for RPC calls.
    ///
    /// Controls how the underlying [`RpcClient`] retries transient failures
    /// (transport errors, timeouts, 5xx). Defaults to
    /// [`RetryConfig::default()`]: 3 retries with 500 ms → 2 s exponential
    /// backoff. This is separate from [`NearBuilder::max_nonce_retries`],
    /// which governs how many times a *transaction* is re-signed and re-sent
    /// after an `InvalidNonce` rejection.
    ///
    /// Callers that own their retry policy (CLIs with a "retry?" prompt,
    /// relayers with their own backoff) can pass [`RetryConfig::none()`] so
    /// near-kit fails fast and leaves the decision to them:
    ///
    /// ```rust
    /// use near_kit::Near;
    /// use near_kit::rpc::RetryConfig;
    ///
    /// let near = Near::testnet()
    ///     .retry_config(RetryConfig::none())
    ///     .build();
    /// ```
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Use a preconfigured HTTP client for RPC requests.
    ///
    /// This allows callers to configure transport concerns such as default
    /// headers, proxies, TLS, and timeouts without expanding near-kit's API for
    /// each individual HTTP setting.
    ///
    /// Not available on WASI, where reqwest doesn't build — see
    /// [`NearBuilder::transport`] for full transport replacement.
    #[cfg(not(all(target_arch = "wasm32", target_os = "wasi")))]
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.transport = Some(Arc::new(transport::ReqwestTransport::with_client(client)));
        self
    }

    /// Replace the HTTP transport entirely with a custom [`RpcTransport`].
    ///
    /// This is the injection point for platforms whose HTTP stack near-kit
    /// doesn't know about — e.g. a runtime that proxies RPC traffic through a
    /// host call instead of exposing raw HTTP. On WASI builds without the
    /// `wasi-http` feature it is the *only* way to reach the network — the
    /// default there only returns a typed transport error. For merely
    /// *configuring* the default reqwest transport (headers, proxies, TLS), prefer
    /// [`NearBuilder::http_client`].
    pub fn transport(mut self, transport: impl RpcTransport + 'static) -> Self {
        self.transport = Some(Arc::new(transport));
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
    ///
    /// On WASI without the `wasi-http` feature, a client built without an
    /// injected transport returns a non-retryable
    /// [`RpcError::Network`](crate::error::RpcError::Network) from RPC calls.
    /// Use [`NearBuilder::transport`] to make that client operational.
    pub fn build(self) -> Near {
        let transport = self.transport.unwrap_or_else(transport::default_transport);
        Near {
            rpc: Arc::new(RpcClient::with_transport_and_retry_config(
                self.rpc_url,
                transport,
                self.retry_config,
            )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InMemorySigner;

    const ALICE_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

    // ========================================================================
    // Near client tests
    // ========================================================================

    #[test]
    fn test_near_mainnet_builder() {
        let near = Near::mainnet().build();
        assert!(near.rpc_url().contains("fastnear") || near.rpc_url().contains("near"));
        assert!(near.chain_id().is_mainnet());
    }

    #[test]
    fn test_near_testnet_builder() {
        let near = Near::testnet().build();
        assert!(near.rpc_url().contains("fastnear") || near.rpc_url().contains("test"));
        assert!(near.chain_id().is_testnet());
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

    #[tokio::test]
    async fn invalid_builder_account_ids_return_errors_instead_of_panicking() {
        let near = Near::testnet().build();

        let query_error = near.balance("INVALID").await.unwrap_err();
        assert!(matches!(query_error, Error::ParseAccountId(_)));

        let global_contract_error = near.global_contract("INVALID").await.unwrap_err();
        assert!(matches!(global_contract_error, Error::ParseAccountId(_)));
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
        let signer = InMemorySigner::new("alice.testnet", ALICE_KEY).unwrap();

        let alice = near.with_signer(signer);
        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(alice.near().rpc_url(), near.rpc_url()); // Same transport
        assert!(Arc::ptr_eq(&alice.near().rpc, &near.rpc));
    }

    #[test]
    fn test_near_with_signer_multiple_accounts() {
        let near = Near::testnet().build();

        let alice = near.with_signer(InMemorySigner::new("alice.testnet", ALICE_KEY).unwrap());
        let bob = near.with_signer(InMemorySigner::new("bob.testnet", ALICE_KEY).unwrap());

        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(bob.account_id().as_str(), "bob.testnet");
        assert!(Arc::ptr_eq(&alice.near().rpc, &bob.near().rpc)); // Shared transport
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

        // A signing client takes the setting of the Near it came from.
        let signer = InMemorySigner::new("alice.testnet", ALICE_KEY).unwrap();
        assert_eq!(near.with_signer(signer).near().max_nonce_retries, 0);
    }
}
