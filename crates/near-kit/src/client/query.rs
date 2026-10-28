//! Awaitable read builders for the [`Near`](crate::Near) reads that add
//! behaviour on top of [`RpcClient`]: unit assembly ([`BalanceQuery`]),
//! error-to-value mapping ([`AccountExistsQuery`]) and typed wait levels
//! ([`TransactionStatusQuery`]). Every other read is an `RpcClient` method.
//!
//! All of them implement `IntoFuture` so they can be `.await`ed directly.

use std::future::IntoFuture;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::{Error, RpcError};
use crate::types::{AccountBalance, AccountId, BlockReference, CryptoHash, Submitted, WaitLevel};

use super::rpc::RpcClient;

/// Reads `account_id` at `block` (final unless [`at`](Self::at) says
/// otherwise).
struct AccountRead {
    rpc: Arc<RpcClient>,
    account_id: AccountId,
    block: BlockReference,
}

impl AccountRead {
    fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            rpc,
            account_id,
            block: BlockReference::default(),
        }
    }
}

// ============================================================================
// BalanceQuery
// ============================================================================

/// An account's balance, ready to `.await`.
///
/// Created by [`Near::balance`](crate::Near::balance). Reads at the final
/// block unless [`at`](Self::at) picks another.
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
///
/// // At a block height, or with another finality
/// let balance = near.balance(account!("alice.testnet")).at(100_000_000).await?;
/// let balance = near.balance(account!("alice.testnet")).at(Finality::Optimistic).await?;
/// # Ok(())
/// # }
/// ```
#[must_use = "queries do nothing unless awaited"]
pub struct BalanceQuery {
    read: AccountRead,
}

impl BalanceQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            read: AccountRead::new(rpc, account_id),
        }
    }

    /// Read at a block height, hash or finality.
    pub fn at(mut self, block: impl Into<BlockReference>) -> Self {
        self.read.block = block.into();
        self
    }
}

impl IntoFuture for BalanceQuery {
    type Output = Result<AccountBalance, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let AccountRead {
            rpc,
            account_id,
            block,
        } = self.read;
        Box::pin(async move {
            let view = rpc.view_account(&account_id, block).await?;
            Ok(AccountBalance::from(view))
        })
    }
}

// ============================================================================
// AccountExistsQuery
// ============================================================================

/// Whether an account exists, ready to `.await`.
///
/// Created by [`Near::account_exists`](crate::Near::account_exists). Reads
/// at the final block unless [`at`](Self::at) picks another.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
///
/// if near.account_exists(account!("alice.testnet")).await? {
///     println!("Account exists!");
/// }
/// # Ok(())
/// # }
/// ```
#[must_use = "queries do nothing unless awaited"]
pub struct AccountExistsQuery {
    read: AccountRead,
}

impl AccountExistsQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, account_id: AccountId) -> Self {
        Self {
            read: AccountRead::new(rpc, account_id),
        }
    }

    /// Read at a block height, hash or finality.
    pub fn at(mut self, block: impl Into<BlockReference>) -> Self {
        self.read.block = block.into();
        self
    }
}

impl IntoFuture for AccountExistsQuery {
    type Output = Result<bool, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let AccountRead {
            rpc,
            account_id,
            block,
        } = self.read;
        Box::pin(async move {
            match rpc.view_account(&account_id, block).await {
                Ok(_) => Ok(true),
                Err(RpcError::AccountNotFound { .. }) => Ok(false),
                Err(e) => Err(e.into()),
            }
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
/// # use near_kit::rpc::{FinalExecutionOutcome, SendTxResponse};
/// # use near_kit::transaction::Final;
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
/// outcome.result()?;
/// # Ok(())
/// # }
/// ```
///
/// Wait levels compose naturally in generic helpers because they are selected
/// entirely in type position:
///
/// ```rust,no_run
/// # use near_kit::*;
/// # use near_kit::transaction::WaitLevel;
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
    sender_id: AccountId,
    _marker: PhantomData<W>,
}

impl TransactionStatusQuery {
    pub(crate) fn new(rpc: Arc<RpcClient>, tx_hash: CryptoHash, sender_id: AccountId) -> Self {
        Self {
            rpc,
            tx_hash,
            sender_id,
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
            let response = self
                .rpc
                .tx_status(&self.tx_hash, &self.sender_id, W::STATUS)
                .await?;
            Ok(W::convert(response, &self.sender_id)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Finality;

    #[test]
    fn reads_default_to_final_and_take_one_block_selector() {
        let rpc = Arc::new(RpcClient::new("http://localhost:3030"));
        let account_id: AccountId = "alice.testnet".parse().unwrap();

        let query = BalanceQuery::new(rpc.clone(), account_id.clone());
        assert_eq!(query.read.block, BlockReference::default());

        let query = BalanceQuery::new(rpc.clone(), account_id.clone()).at(12345);
        assert_eq!(query.read.block, BlockReference::Height(12345));

        let query = AccountExistsQuery::new(rpc, account_id).at(Finality::Optimistic);
        assert_eq!(
            query.read.block,
            BlockReference::Finality(Finality::Optimistic)
        );
    }
}
