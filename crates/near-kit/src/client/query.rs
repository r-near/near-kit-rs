//! Query builders for fluent read operations.
//!
//! All query builders implement `IntoFuture` so they can be `.await`ed directly.

use std::future::IntoFuture;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::types::{
    AccessKeyListView, AccountBalance, AccountId, AccountView, BlockReference, ContractCodeView,
    CryptoHash, Finality, GlobalContractId, PublicKeyHandle, Submitted, TryIntoAccountId,
    WaitLevel,
};

use super::rpc::RpcClient;

// ============================================================================
// BalanceQuery
// ============================================================================

/// Query builder for getting account balance.
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
///
/// // Query at specific block
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
pub struct BalanceQuery {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl BalanceQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }
}

impl IntoFuture for BalanceQuery {
    type Output = Result<AccountBalance, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let view = self
                .rpc
                .view_account(&self.account_id, self.block_ref)
                .await?;
            Ok(AccountBalance::from(view))
        })
    }
}

// ============================================================================
// AccountQuery
// ============================================================================

/// Query builder for getting full account information.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// let account = near.account("alice.testnet").await?;
/// println!("Storage used: {} bytes", account.storage_usage);
/// # Ok(())
/// # }
/// ```
pub struct AccountQuery {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl AccountQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }
}

impl IntoFuture for AccountQuery {
    type Output = Result<AccountView, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let view = self
                .rpc
                .view_account(&self.account_id, self.block_ref)
                .await?;
            Ok(view)
        })
    }
}

// ============================================================================
// AccountExistsQuery
// ============================================================================

/// Query builder for checking if an account exists.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// if near.account_exists("alice.testnet").await? {
///     println!("Account exists!");
/// }
/// # Ok(())
/// # }
/// ```
pub struct AccountExistsQuery {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl AccountExistsQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }
}

impl IntoFuture for AccountExistsQuery {
    type Output = Result<bool, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            match self
                .rpc
                .view_account(&self.account_id, self.block_ref)
                .await
            {
                Ok(_) => Ok(true),
                Err(crate::error::RpcError::AccountNotFound { .. }) => Ok(false),
                Err(e) => Err(e.into()),
            }
        })
    }
}

// ============================================================================
// AccessKeysQuery
// ============================================================================

/// Query builder for listing access keys.
///
/// Awaiting it returns *every* key: the node caps one response at 100 keys by
/// default, so the query pages through [`RpcClient::view_access_key_list`]
/// transparently. Use [`page`](Self::page) to fetch one page at a time.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// let keys = near.access_keys("alice.testnet").await?;
/// for key_info in keys.keys {
///     println!("Key: {}", key_info.public_key);
/// }
/// # Ok(())
/// # }
/// ```
pub struct AccessKeysQuery {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl AccessKeysQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }

    /// Fetch a single page instead of the whole list.
    ///
    /// `after_key` is the previous page's
    /// [`last_key`](AccessKeyListView::last_key) and `limit` the page size;
    /// see [`RpcClient::view_access_key_list_page`] for the semantics,
    /// including [`RpcError::TooManyAccessKeys`](crate::RpcError::TooManyAccessKeys)
    /// when both are `None` and the account is over the cap.
    pub async fn page(
        self,
        after_key: Option<&PublicKeyHandle>,
        limit: Option<NonZeroU32>,
    ) -> Result<AccessKeyListView, Error> {
        Ok(self
            .rpc
            .view_access_key_list_page(&self.account_id, after_key, limit, self.block_ref)
            .await?)
    }
}

impl IntoFuture for AccessKeysQuery {
    type Output = Result<AccessKeyListView, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let list = self
                .rpc
                .view_access_key_list(&self.account_id, self.block_ref)
                .await?;
            Ok(list)
        })
    }
}

// ============================================================================
// TransactionStatusQuery
// ============================================================================

/// Awaitable query for transaction execution status.
///
/// Awaiting this query directly uses [`Submitted`], which asks the node for its
/// current progress without waiting for a new execution milestone. Use
/// [`wait_until`](Self::wait_until) to block until a specific type-safe level.
/// The selected wait-level type also determines the response type.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example(
/// #     near: &Near,
/// #     tx_hash: &CryptoHash,
/// #     sender_id: &AccountId,
/// # ) -> Result<(), Error> {
/// // The default is a non-blocking progress query.
/// let progress: SendTxResponse = near.tx_status(tx_hash, sender_id).await?;
///
/// // Waiting for execution returns a full execution outcome.
/// let outcome: FinalExecutionOutcome = near
///     .tx_status(tx_hash, sender_id)
///     .wait_until::<Final>()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// Wait levels compose naturally in generic helpers because they are selected
/// entirely in type position:
///
/// ```rust,no_run
/// # use near_kit::*;
/// async fn status_at<W: WaitLevel>(
///     near: &Near,
///     tx_hash: &CryptoHash,
///     sender_id: &AccountId,
/// ) -> Result<W::Response, Error> {
///     near.tx_status(tx_hash, sender_id).wait_until::<W>().await
/// }
/// ```
#[must_use = "transaction status queries do nothing unless awaited"]
pub struct TransactionStatusQuery<W: WaitLevel = Submitted> {
    rpc: Arc<RpcClient>,
    tx_hash: CryptoHash,
    sender_id: Result<AccountId, Error>,
    _marker: PhantomData<W>,
}

impl TransactionStatusQuery {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        tx_hash: CryptoHash,
        sender_id: impl TryIntoAccountId,
    ) -> Self {
        Self {
            rpc,
            tx_hash,
            sender_id: sender_id.try_into_account_id().map_err(Error::from),
            _marker: PhantomData,
        }
    }
}

impl<W: WaitLevel> TransactionStatusQuery<W> {
    /// Select the execution wait level and corresponding response type.
    pub fn wait_until<W2: WaitLevel>(self) -> TransactionStatusQuery<W2> {
        TransactionStatusQuery {
            rpc: self.rpc,
            tx_hash: self.tx_hash,
            sender_id: self.sender_id,
            _marker: PhantomData,
        }
    }
}

impl<W: WaitLevel> IntoFuture for TransactionStatusQuery<W> {
    type Output = Result<W::Response, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let sender_id = self.sender_id?;
            let response = self
                .rpc
                .tx_status(&self.tx_hash, &sender_id, W::STATUS)
                .await?;
            W::convert(response, &sender_id)
        })
    }
}

// ============================================================================
// ViewCall
// ============================================================================

/// Query builder for calling view functions on contracts.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// // Simple view call without args
/// let count: u64 = near.view("counter.testnet", "get_count").await?;
///
/// // View call with args
/// let messages: Vec<String> = near.view("guestbook.testnet", "get_messages")
///     .args(serde_json::json!({ "limit": 10 }))
///     .await?;
///
/// // Query at specific block
/// let old_count: u64 = near.view("counter.testnet", "get_count")
///     .at_block(100_000_000)
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ViewCall<T> {
    rpc: Arc<RpcClient>,
    contract_id: AccountId,
    method: String,
    args: Vec<u8>,
    block_ref: BlockReference,
    _phantom: PhantomData<T>,
}

impl<T> ViewCall<T> {
    pub(crate) fn new(rpc: Arc<RpcClient>, contract_id: AccountId, method: String) -> Self {
        Self {
            rpc,
            contract_id,
            method,
            args: vec![],
            block_ref: BlockReference::default(),
            _phantom: PhantomData,
        }
    }

    /// Set JSON arguments for the view call.
    ///
    /// The arguments will be serialized to JSON.
    pub fn args<A: serde::Serialize>(mut self, args: A) -> Self {
        self.args = serde_json::to_vec(&args).unwrap_or_default();
        self
    }

    /// Set raw byte arguments (e.g., Borsh encoded).
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.args = args;
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh<A: borsh::BorshSerialize>(mut self, args: A) -> Self {
        self.args = borsh::to_vec(&args).unwrap_or_default();
        self
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }

    /// Switch to Borsh deserialization for the response.
    ///
    /// By default, `ViewCall` deserializes responses as JSON. Call this method
    /// to deserialize as Borsh instead. This is useful for contracts that return
    /// Borsh-encoded data.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::*;
    /// use borsh::BorshDeserialize;
    ///
    /// #[derive(BorshDeserialize)]
    /// struct ContractState { count: u64 }
    ///
    /// async fn example() -> Result<(), near_kit::Error> {
    ///     let near = Near::testnet().build();
    ///
    ///     // Borsh response deserialization
    ///     let state: ContractState = near.view("contract.testnet", "get_state")
    ///         .borsh()
    ///         .await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn borsh(self) -> ViewCallBorsh<T> {
        ViewCallBorsh {
            rpc: self.rpc,
            contract_id: self.contract_id,
            method: self.method,
            args: self.args,
            block_ref: self.block_ref,
            _phantom: PhantomData,
        }
    }
}

impl<T: DeserializeOwned + Send + 'static> IntoFuture for ViewCall<T> {
    type Output = Result<T, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let result = self
                .rpc
                .view_function(&self.contract_id, &self.method, &self.args, self.block_ref)
                .await?;
            Ok(result.json()?)
        })
    }
}

// ============================================================================
// ViewCallBorsh
// ============================================================================

/// Query builder for view functions with Borsh deserialization.
///
/// Created by calling [`.borsh()`](ViewCall::borsh) on a `ViewCall`.
/// This variant deserializes the response as Borsh instead of JSON.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
/// use borsh::BorshDeserialize;
///
/// #[derive(BorshDeserialize)]
/// struct ContractState { count: u64 }
///
/// #[derive(borsh::BorshSerialize)]
/// struct MyArgs { key: u64 }
///
/// async fn example() -> Result<(), near_kit::Error> {
///     let near = Near::testnet().build();
///
///     // JSON args, Borsh response
///     let state: ContractState = near.view("contract.testnet", "get_state")
///         .args(serde_json::json!({ "key": "value" }))
///         .borsh()
///         .await?;
///
///     // Borsh args, Borsh response
///     let state: ContractState = near.view("contract.testnet", "get_state")
///         .args_borsh(MyArgs { key: 123 })
///         .borsh()
///         .await?;
///     Ok(())
/// }
/// ```
pub struct ViewCallBorsh<T> {
    rpc: Arc<RpcClient>,
    contract_id: AccountId,
    method: String,
    args: Vec<u8>,
    block_ref: BlockReference,
    _phantom: PhantomData<T>,
}

impl<T: borsh::BorshDeserialize + Send + 'static> IntoFuture for ViewCallBorsh<T> {
    type Output = Result<T, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let result = self
                .rpc
                .view_function(&self.contract_id, &self.method, &self.args, self.block_ref)
                .await?;
            result.borsh().map_err(|e| Error::Borsh(e.to_string()))
        })
    }
}

// ============================================================================
// ContractCodeQuery
// ============================================================================

/// Query builder for fetching the WASM code deployed on an account.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// let contract = near.contract_code("app.near").await?;
/// println!(
///     "code hash: {}, {} bytes, read at block {}",
///     contract.hash,
///     contract.code.len(),
///     contract.block_height
/// );
///
/// // Check whether a contract is deployed at all
/// if near.contract_code("app.near").exists().await? {
///     println!("Contract deployed!");
/// }
/// # Ok(())
/// # }
/// ```
pub struct ContractCodeQuery {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl ContractCodeQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }

    /// Check whether a contract is deployed, instead of fetching its code.
    ///
    /// Returns `Ok(false)` when the account has no contract or does not
    /// exist. Note the node still returns the full code on the wire — the
    /// RPC has no lighter existence check.
    pub async fn exists(self) -> Result<bool, Error> {
        match self.rpc.view_code(&self.account_id, self.block_ref).await {
            Ok(_) => Ok(true),
            Err(crate::error::RpcError::ContractNotDeployed { .. })
            | Err(crate::error::RpcError::AccountNotFound { .. }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

impl IntoFuture for ContractCodeQuery {
    type Output = Result<ContractCodeView, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let view = self.rpc.view_code(&self.account_id, self.block_ref).await?;
            Ok(view)
        })
    }
}

// ============================================================================
// GlobalContractQuery
// ============================================================================

/// Query builder for fetching a global contract's code and hash.
///
/// Accepts the same identifiers as `deploy_from`: a publisher account ID
/// (updatable contracts) or a code hash (immutable contracts).
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example(code_hash: CryptoHash) -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// // By publisher account (updatable) — `hash` is the current version
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
pub struct GlobalContractQuery {
    rpc: Arc<RpcClient>,
    id: GlobalContractId,
    block_ref: BlockReference,
}

impl GlobalContractQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, id: GlobalContractId) -> Self {
        Self {
            rpc,
            id,
            block_ref: BlockReference::default(),
        }
    }

    /// Query at a specific block height.
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }

    /// Query at a specific block hash.
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }

    /// Query with specific finality.
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }

    /// Check whether the global contract is deployed, instead of fetching
    /// its code.
    ///
    /// Returns `Ok(false)` when nothing is published under the identifier,
    /// including when a publisher account does not exist. Note the node
    /// still returns the full code on the wire — the RPC has no lighter
    /// existence check.
    pub async fn exists(self) -> Result<bool, Error> {
        match self
            .rpc
            .view_global_contract_code(&self.id, self.block_ref)
            .await
        {
            Ok(_) => Ok(true),
            // AccountNotFound isn't returned by current nodes (the lookup is
            // by identifier, not account), but a nonexistent publisher has
            // published nothing — treat it the same as not-found.
            Err(crate::error::RpcError::GlobalContractNotFound { .. })
            | Err(crate::error::RpcError::AccountNotFound { .. }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

impl IntoFuture for GlobalContractQuery {
    type Output = Result<ContractCodeView, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let view = self
                .rpc
                .view_global_contract_code(&self.id, self.block_ref)
                .await?;
            Ok(view)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_query_builder() {
        let rpc = Arc::new(RpcClient::new("http://localhost:3030"));
        let account_id: AccountId = "alice.testnet".parse().unwrap();

        let query = BalanceQuery::new(rpc.clone(), account_id.clone());
        assert_eq!(query.block_ref, BlockReference::default());

        let query = BalanceQuery::new(rpc.clone(), account_id.clone()).at_block(12345);
        assert_eq!(query.block_ref, BlockReference::Height(12345));

        let query = BalanceQuery::new(rpc.clone(), account_id).finality(Finality::Optimistic);
        assert_eq!(
            query.block_ref,
            BlockReference::Finality(Finality::Optimistic)
        );
    }
}
