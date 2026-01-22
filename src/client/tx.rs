//! Transaction builders for fluent write operations.
//!
//! All transaction builders implement `IntoFuture` so they can be `.await`ed directly.

use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Error;
use crate::types::{
    AccountId, Action, BlockReference, FinalExecutionOutcome, Finality, Gas, IntoGas,
    IntoNearToken, NearToken, PublicKey, Transaction, TxExecutionStatus,
};

use super::rpc::RpcClient;
use super::signer::Signer;

// ============================================================================
// TransferCall
// ============================================================================

/// Builder for transferring NEAR tokens.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::prelude::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///         .credentials("ed25519:...", "alice.testnet").unwrap()
///     .build();
///
/// // Simple transfer
/// near.transfer("bob.testnet", "1 NEAR").await?;
///
/// // Transfer with custom wait status
/// near.transfer("bob.testnet", "1000 NEAR")
///     .wait_until(TxExecutionStatus::Final)
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct TransferCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    receiver_id: AccountId,
    amount: NearToken,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl TransferCall {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        receiver_id: AccountId,
        amount: NearToken,
    ) -> Self {
        Self {
            rpc,
            signer,
            receiver_id,
            amount,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }

    /// Set the execution wait level.
    ///
    /// Controls how long the RPC waits before returning.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for TransferCall {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    signer.public_key(),
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;

            // Get recent block hash
            let block = self
                .rpc
                .block(BlockReference::Finality(Finality::Final))
                .await?;

            // Build transaction
            let tx = Transaction::new(
                signer_id,
                signer.public_key().clone(),
                access_key.nonce + 1,
                self.receiver_id,
                block.header.hash,
                vec![Action::transfer(self.amount)],
            );

            // Sign
            let signature = signer.sign(tx.get_hash().as_bytes())?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let outcome = self.rpc.send_tx(&signed_tx, self.wait_until).await?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            Ok(outcome)
        })
    }
}

// ============================================================================
// ContractCall
// ============================================================================

/// Builder for calling functions on contracts.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::prelude::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///         .credentials("ed25519:...", "alice.testnet").unwrap()
///     .build();
///
/// // Simple call without args
/// near.call("counter.testnet", "increment").await?;
///
/// // Call with args, gas, and deposit
/// near.call("nft.testnet", "nft_mint")
///     .args(serde_json::json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
///     .gas("100 Tgas")
///     .deposit("0.1 NEAR")
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ContractCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    contract_id: AccountId,
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl ContractCall {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        contract_id: AccountId,
        method: String,
    ) -> Self {
        Self {
            rpc,
            signer,
            contract_id,
            method,
            args: vec![],
            gas: Gas::DEFAULT,
            deposit: NearToken::ZERO,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Set JSON arguments.
    pub fn args<A: serde::Serialize>(mut self, args: A) -> Self {
        self.args = serde_json::to_vec(&args).unwrap_or_default();
        self
    }

    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.args = args;
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh<A: borsh::BorshSerialize>(mut self, args: A) -> Self {
        self.args = borsh::to_vec(&args).unwrap_or_default();
        self
    }

    /// Set gas limit (accepts string like "30 Tgas" or Gas type).
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        if let Ok(g) = gas.into_gas() {
            self.gas = g;
        }
        self
    }

    /// Set attached deposit (accepts string like "1 NEAR" or NearToken type).
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        if let Ok(a) = amount.into_near_token() {
            self.deposit = a;
        }
        self
    }

    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }

    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for ContractCall {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    signer.public_key(),
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;

            // Get recent block hash
            let block = self
                .rpc
                .block(BlockReference::Finality(Finality::Final))
                .await?;

            // Build transaction
            let tx = Transaction::new(
                signer_id,
                signer.public_key().clone(),
                access_key.nonce + 1,
                self.contract_id,
                block.header.hash,
                vec![Action::function_call(
                    self.method,
                    self.args,
                    self.gas,
                    self.deposit,
                )],
            );

            // Sign
            let signature = signer.sign(tx.get_hash().as_bytes())?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let outcome = self.rpc.send_tx(&signed_tx, self.wait_until).await?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            Ok(outcome)
        })
    }
}

// ============================================================================
// DeployCall
// ============================================================================

/// Builder for deploying contracts.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::prelude::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///         .credentials("ed25519:...", "alice.testnet").unwrap()
///     .build();
///
/// let wasm_code = std::fs::read("contract.wasm").unwrap();
/// near.deploy("alice.testnet", wasm_code).await?;
/// # Ok(())
/// # }
/// ```
pub struct DeployCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    account_id: AccountId,
    code: Vec<u8>,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl DeployCall {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        account_id: AccountId,
        code: Vec<u8>,
    ) -> Self {
        Self {
            rpc,
            signer,
            account_id,
            code,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }

    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for DeployCall {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    signer.public_key(),
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;

            // Get recent block hash
            let block = self
                .rpc
                .block(BlockReference::Finality(Finality::Final))
                .await?;

            // Build transaction
            let tx = Transaction::new(
                signer_id,
                signer.public_key().clone(),
                access_key.nonce + 1,
                self.account_id,
                block.header.hash,
                vec![Action::deploy_contract(self.code)],
            );

            // Sign
            let signature = signer.sign(tx.get_hash().as_bytes())?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let outcome = self.rpc.send_tx(&signed_tx, self.wait_until).await?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            Ok(outcome)
        })
    }
}

// ============================================================================
// AddKeyCall
// ============================================================================

/// Builder for adding access keys.
pub struct AddKeyCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    account_id: AccountId,
    public_key: PublicKey,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl AddKeyCall {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        account_id: AccountId,
        public_key: PublicKey,
    ) -> Self {
        Self {
            rpc,
            signer,
            account_id,
            public_key,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }

    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for AddKeyCall {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    signer.public_key(),
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;

            // Get recent block hash
            let block = self
                .rpc
                .block(BlockReference::Finality(Finality::Final))
                .await?;

            // Build transaction
            let tx = Transaction::new(
                signer_id,
                signer.public_key().clone(),
                access_key.nonce + 1,
                self.account_id,
                block.header.hash,
                vec![Action::add_full_access_key(self.public_key)],
            );

            // Sign
            let signature = signer.sign(tx.get_hash().as_bytes())?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let outcome = self.rpc.send_tx(&signed_tx, self.wait_until).await?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            Ok(outcome)
        })
    }
}

// ============================================================================
// DeleteKeyCall
// ============================================================================

/// Builder for deleting access keys.
pub struct DeleteKeyCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    account_id: AccountId,
    public_key: PublicKey,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl DeleteKeyCall {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        account_id: AccountId,
        public_key: PublicKey,
    ) -> Self {
        Self {
            rpc,
            signer,
            account_id,
            public_key,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }

    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for DeleteKeyCall {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    signer.public_key(),
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;

            // Get recent block hash
            let block = self
                .rpc
                .block(BlockReference::Finality(Finality::Final))
                .await?;

            // Build transaction
            let tx = Transaction::new(
                signer_id,
                signer.public_key().clone(),
                access_key.nonce + 1,
                self.account_id,
                block.header.hash,
                vec![Action::delete_key(self.public_key)],
            );

            // Sign
            let signature = signer.sign(tx.get_hash().as_bytes())?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let outcome = self.rpc.send_tx(&signed_tx, self.wait_until).await?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            Ok(outcome)
        })
    }
}
