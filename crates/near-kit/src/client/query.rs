//! Query builders for fluent read operations.
//!
//! All query builders implement `IntoFuture` so they can be `.await`ed directly.

use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::types::{
    AccessKeyListView, AccountBalance, AccountId, AccountView, BlockReference, CryptoHash, Finality,
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
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

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
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

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
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            match self
                .rpc
                .view_account(&self.account_id, self.block_ref)
                .await
            {
                Ok(_) => Ok(true),
                Err(crate::error::RpcError::AccountNotFound(_)) => Ok(false),
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
}

impl IntoFuture for AccessKeysQuery {
    type Output = Result<AccessKeyListView, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

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
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

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
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

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
