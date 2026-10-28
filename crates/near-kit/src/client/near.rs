//! The main Near client.

use std::future::Future;
use std::sync::Arc;

use crate::error::RpcError;
use crate::platform::MaybeSend;
use serde::de::DeserializeOwned;

use crate::transaction::View;
use crate::types::{AccountId, BlockReference, ChainId, ViewFunctionResult};

use super::query::{AccountExistsQuery, BalanceQuery, TransactionStatusQuery};
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
///     let balance = near.balance(account!("alice.testnet")).await?;
///     println!("Balance: {}", balance);
///
///     let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?));
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

    /// Get the underlying RPC client.
    pub fn rpc(&self) -> &RpcClient {
        &self.rpc
    }

    /// Get the chain ID this client is connected to.
    pub fn chain_id(&self) -> &ChainId {
        &self.chain_id
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
    /// let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?));
    /// let bob = near.with_signer(InMemorySigner::new(account!("bob.testnet"), "ed25519:...".parse()?));
    /// assert_eq!(alice.account_id().as_str(), "alice.testnet");
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_signer(&self, signer: impl Signer + 'static) -> SigningNear {
        SigningNear::new(self.clone(), Arc::new(signer))
    }

    // ========================================================================
    // Reads
    // ========================================================================
    //
    // Only reads that add something to the matching `RpcClient` method live
    // here; for everything else use `near.rpc()` (e.g.
    // `near.rpc().view_account(&id, block)`).

    /// Get an account's balance: total, available, locked and the amount
    /// reserved for storage.
    ///
    /// Assembles an [`AccountBalance`](crate::rpc::AccountBalance) from the
    /// account view and the storage cost. Reads at the final block unless
    /// [`at`](BalanceQuery::at) picks another.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::rpc::Finality;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// let balance = near.balance(account!("alice.testnet")).await?;
    /// println!("Available: {}", balance.available);
    ///
    /// // At a specific block height, or with another finality
    /// let balance = near.balance(account!("alice.testnet")).at(100_000_000).await?;
    /// let balance = near.balance(account!("alice.testnet")).at(Finality::Optimistic).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn balance(&self, account_id: impl Into<AccountId>) -> BalanceQuery {
        BalanceQuery::new(self.rpc.clone(), account_id.into())
    }

    /// Check whether an account exists.
    ///
    /// Maps [`RpcError::AccountNotFound`] to `false`. For the account itself,
    /// use `near.rpc().view_account(&id, block)`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// if near.account_exists(account!("alice.testnet")).await? {
    ///     println!("Account exists!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn account_exists(&self, account_id: impl Into<AccountId>) -> AccountExistsQuery {
        AccountExistsQuery::new(self.rpc.clone(), account_id.into())
    }

    /// Call a view function and decode its JSON result.
    ///
    /// Returns the same [`View`] a `#[near_kit::contract]` client returns:
    /// set arguments with [`args`](View::args) (JSON),
    /// [`args_borsh`](View::args_borsh) or [`args_raw`](View::args_raw), pick
    /// a block with [`at`](View::at), and `.await` it. It sends no arguments
    /// unless one of those sets them. For a Borsh-encoded result, build the
    /// view with [`View::borsh`] instead.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::transaction::View;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    ///
    /// let count: u64 = near.view(account!("counter.testnet"), "get_count").await?;
    ///
    /// let messages: Vec<String> = near
    ///     .view(account!("guestbook.testnet"), "get_messages")
    ///     .args(serde_json::json!({ "limit": 10 }))
    ///     .await?;
    ///
    /// #[derive(borsh::BorshDeserialize)]
    /// struct State { count: u64 }
    /// let state: State = View::borsh(near.clone(), account!("counter.testnet"), "get_state").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn view<T: DeserializeOwned>(
        &self,
        contract_id: impl Into<AccountId>,
        method: impl Into<String>,
    ) -> View<Near, T> {
        View::json(self.clone(), contract_id, method)
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
    /// // Send later. Like a `Tx`, it waits for `Final` unless you pick
    /// // another level with `.wait_until::<W>()`.
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
    ///     .tx_status(&tx_hash, account!("alice.testnet"))
    ///     .wait_until::<Final>()
    ///     .await?;
    /// println!("Gas used: {}", outcome.total_gas_used());
    /// println!("Receipts: {}", outcome.receipts.len());
    ///
    /// // Poll current progress without blocking:
    /// let status = near.tx_status(&tx_hash, account!("alice.testnet")).await?;
    /// if let Some(partial) = &status.outcome {
    ///     println!("Receipts so far: {}", partial.receipts_outcome.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn tx_status(
        &self,
        tx_hash: &crate::types::CryptoHash,
        sender_id: impl Into<AccountId>,
    ) -> TransactionStatusQuery {
        TransactionStatusQuery::new(self.rpc.clone(), *tx_hash, sender_id.into())
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
    /// A `Near`-backed client can only view; for `#[call]` methods, use
    /// [`SigningNear::contract`](crate::SigningNear::contract).
    ///
    /// # Example
    ///
    #[cfg_attr(feature = "contracts", doc = "```rust,no_run")]
    #[cfg_attr(not(feature = "contracts"), doc = "```rust,ignore")]
    /// use near_kit::*;
    ///
    /// #[near_kit::contract]
    /// pub trait Counter {
    ///     fn get_count(&self) -> u64;
    /// }
    ///
    /// async fn example(near: &Near) -> Result<(), near_kit::Error> {
    ///     let counter = near.contract::<Counter>(account!("counter.testnet"));
    ///     let count = counter.get_count().await?;
    ///     let earlier = counter.get_count().at(180_000_000).await?;
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

    /// Replace the HTTP transport entirely with a custom [`RpcTransport`].
    ///
    /// This is the injection point for platforms whose HTTP stack near-kit
    /// doesn't know about — e.g. a runtime that proxies RPC traffic through a
    /// host call instead of exposing raw HTTP. On WASI builds without the
    /// `wasi-http` feature it is the *only* way to reach the network — the
    /// default there only returns a typed transport error. To configure the
    /// default reqwest transport (headers, proxies, TLS, timeouts), pass
    /// `ReqwestTransport::with_client(client)`.
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
    use crate::account;
    use crate::types::InMemorySigner;

    const ALICE_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

    // ========================================================================
    // Near client tests
    // ========================================================================

    #[test]
    fn test_near_mainnet_builder() {
        let near = Near::mainnet().build();
        assert!(near.rpc().url().contains("fastnear") || near.rpc().url().contains("near"));
        assert!(near.chain_id().is_mainnet());
    }

    #[test]
    fn test_near_testnet_builder() {
        let near = Near::testnet().build();
        assert!(near.rpc().url().contains("fastnear") || near.rpc().url().contains("test"));
        assert!(near.chain_id().is_testnet());
    }

    #[test]
    fn test_near_custom_builder() {
        let near = Near::custom("https://custom-rpc.example.com", "mainnet").build();
        assert_eq!(near.rpc().url(), "https://custom-rpc.example.com");
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

    // ========================================================================
    // NearBuilder tests
    // ========================================================================

    #[test]
    fn test_near_builder_new() {
        let builder = NearBuilder::new("https://example.com", ChainId::new("custom"));
        let near = builder.build();
        assert_eq!(near.rpc().url(), "https://example.com");
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
        assert!(!near.rpc().url().is_empty());
    }

    #[test]
    fn test_near_builder_from_trait() {
        let builder = Near::testnet();
        let near: Near = builder.into();
        assert!(!near.rpc().url().is_empty());
    }

    // ========================================================================
    // Clone tests
    // ========================================================================

    #[test]
    fn test_near_clone() {
        let near1 = Near::testnet().build();
        let near2 = near1.clone();
        assert_eq!(near1.rpc().url(), near2.rpc().url());
    }

    #[test]
    fn test_near_with_signer_derived() {
        let near = Near::testnet().build();
        let signer = InMemorySigner::new(account!("alice.testnet"), ALICE_KEY.parse().unwrap());

        let alice = near.with_signer(signer);
        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(alice.near().rpc().url(), near.rpc().url()); // Same transport
        assert!(Arc::ptr_eq(&alice.near().rpc, &near.rpc));
    }

    #[test]
    fn test_near_with_signer_multiple_accounts() {
        let near = Near::testnet().build();

        let alice = near.with_signer(InMemorySigner::new(
            account!("alice.testnet"),
            ALICE_KEY.parse().unwrap(),
        ));
        let bob = near.with_signer(InMemorySigner::new(
            account!("bob.testnet"),
            ALICE_KEY.parse().unwrap(),
        ));

        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(bob.account_id().as_str(), "bob.testnet");
        assert!(Arc::ptr_eq(&alice.near().rpc, &bob.near().rpc)); // Shared transport
    }

    #[test]
    fn test_near_max_nonce_retries() {
        let near = Near::testnet().build();
        assert_eq!(near.max_nonce_retries, 3);

        let near = Near::testnet().max_nonce_retries(10).build();
        assert_eq!(near.max_nonce_retries, 10);

        // 0 means no retries (send once)
        let near = Near::testnet().max_nonce_retries(0).build();
        assert_eq!(near.max_nonce_retries, 0);

        // A signing client takes the setting of the Near it came from.
        let signer = InMemorySigner::new(account!("alice.testnet"), ALICE_KEY.parse().unwrap());
        assert_eq!(near.with_signer(signer).near().max_nonce_retries, 0);
    }
}
