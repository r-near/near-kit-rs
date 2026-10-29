//! Awaitable builders for the [`Near`](crate::Near) reads that add something to [`RpcClient`].

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

/// An account's balance, ready to `.await`.
///
/// Created by [`Near::balance`](crate::Near::balance). Reads at the final
/// block unless [`at`](Self::at) picks another.
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
    type IntoFuture = super::transport::BoxFuture<'static, Self::Output>;

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

/// Whether an account exists, ready to `.await`.
///
/// Created by [`Near::account_exists`](crate::Near::account_exists). Reads
/// at the final block unless [`at`](Self::at) picks another.
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
    type IntoFuture = super::transport::BoxFuture<'static, Self::Output>;

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

/// Awaitable query for transaction execution status.
///
/// Created by [`Near::tx_status`](crate::Near::tx_status). Awaiting it
/// directly uses [`Submitted`], which asks the node for its current progress
/// without waiting for a new execution milestone. Use
/// [`wait_until`](Self::wait_until) to block until a specific type-safe level.
/// The selected wait-level type also determines the response type.
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
    type IntoFuture = super::transport::BoxFuture<'static, Self::Output>;

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
