//! The main Near client.

use std::sync::Arc;

use crate::error::Error;
use crate::types::{
    AccountBalance, AccountId, AccountView, AccessKeyListView, BlockReference,
    Finality, FinalExecutionOutcome, Gas, NearToken, PublicKey, Transaction, TxExecutionStatus,
    Action,
};

use super::rpc::{RpcClient, RetryConfig, MAINNET, TESTNET};
use super::signer::Signer;

/// The main client for interacting with NEAR Protocol.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::prelude::*;
///
/// #[tokio::main]
/// async fn main() -> Result<(), near_kit::Error> {
///     let near = Near::testnet().build();
///     
///     let balance = near.balance("alice.testnet").await?;
///     println!("Balance: {}", balance);
///     
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Near {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    default_account: Option<AccountId>,
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

    // ========================================================================
    // Read Operations
    // ========================================================================

    /// Get account balance.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let balance = near.balance("alice.testnet").await?;
    /// println!("Available: {}", balance.available);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn balance(&self, account_id: impl AsRef<str>) -> Result<AccountBalance, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let view = self
            .rpc
            .view_account(&account_id, BlockReference::default())
            .await?;
        Ok(AccountBalance::from(view))
    }

    /// Get account balance at a specific block.
    pub async fn balance_at(
        &self,
        account_id: impl AsRef<str>,
        block: impl Into<BlockReference>,
    ) -> Result<AccountBalance, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let view = self.rpc.view_account(&account_id, block.into()).await?;
        Ok(AccountBalance::from(view))
    }

    /// Get full account information.
    pub async fn account(&self, account_id: impl AsRef<str>) -> Result<AccountView, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let view = self
            .rpc
            .view_account(&account_id, BlockReference::default())
            .await?;
        Ok(view)
    }

    /// Check if an account exists.
    pub async fn account_exists(&self, account_id: impl AsRef<str>) -> Result<bool, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        match self
            .rpc
            .view_account(&account_id, BlockReference::default())
            .await
        {
            Ok(_) => Ok(true),
            Err(crate::error::RpcError::AccountNotFound(_)) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Call a view function on a contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let count: u64 = near.view("counter.testnet", "get_count").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn view<T: serde::de::DeserializeOwned>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
    ) -> Result<T, Error> {
        self.view_with_args(contract_id, method, &()).await
    }

    /// Call a view function with arguments.
    pub async fn view_with_args<T: serde::de::DeserializeOwned, A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
    ) -> Result<T, Error> {
        let contract_id: AccountId = contract_id.as_ref().parse()?;
        let args_bytes = serde_json::to_vec(args)?;
        let result = self
            .rpc
            .view_function(&contract_id, method, &args_bytes, BlockReference::default())
            .await?;
        Ok(result.json()?)
    }

    /// Get all access keys for an account.
    pub async fn access_keys(
        &self,
        account_id: impl AsRef<str>,
    ) -> Result<AccessKeyListView, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        let list = self
            .rpc
            .view_access_key_list(&account_id, BlockReference::default())
            .await?;
        Ok(list)
    }

    // ========================================================================
    // Write Operations
    // ========================================================================

    /// Transfer NEAR tokens.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .signer(SecretKeySigner::generate())
    ///     .default_account("alice.testnet")
    ///     .build();
    ///
    /// near.transfer("bob.testnet", "1 NEAR").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn transfer(
        &self,
        receiver: impl AsRef<str>,
        amount: impl AsRef<str>,
    ) -> Result<FinalExecutionOutcome, Error> {
        let receiver_id: AccountId = receiver.as_ref().parse()?;
        let amount: NearToken = amount.as_ref().parse()?;

        self.send_actions(receiver_id, vec![Action::transfer(amount)])
            .await
    }

    /// Call a function on a contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .signer(SecretKeySigner::generate())
    ///     .default_account("alice.testnet")
    ///     .build();
    ///
    /// near.call("counter.testnet", "increment").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
    ) -> Result<FinalExecutionOutcome, Error> {
        self.call_with_args(contract_id, method, &()).await
    }

    /// Call a function with arguments.
    pub async fn call_with_args<A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
    ) -> Result<FinalExecutionOutcome, Error> {
        self.call_with_options(contract_id, method, args, Gas::DEFAULT, NearToken::ZERO)
            .await
    }

    /// Call a function with full options.
    pub async fn call_with_options<A: serde::Serialize>(
        &self,
        contract_id: impl AsRef<str>,
        method: &str,
        args: &A,
        gas: Gas,
        deposit: NearToken,
    ) -> Result<FinalExecutionOutcome, Error> {
        let contract_id: AccountId = contract_id.as_ref().parse()?;
        let args_bytes = serde_json::to_vec(args)?;

        self.send_actions(
            contract_id,
            vec![Action::function_call(method, args_bytes, gas, deposit)],
        )
        .await
    }

    /// Deploy a contract.
    pub async fn deploy(
        &self,
        account_id: impl AsRef<str>,
        code: Vec<u8>,
    ) -> Result<FinalExecutionOutcome, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        self.send_actions(account_id, vec![Action::deploy_contract(code)])
            .await
    }

    /// Add a full access key to an account.
    pub async fn add_full_access_key(
        &self,
        account_id: impl AsRef<str>,
        public_key: PublicKey,
    ) -> Result<FinalExecutionOutcome, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        self.send_actions(account_id, vec![Action::add_full_access_key(public_key)])
            .await
    }

    /// Delete an access key from an account.
    pub async fn delete_key(
        &self,
        account_id: impl AsRef<str>,
        public_key: PublicKey,
    ) -> Result<FinalExecutionOutcome, Error> {
        let account_id: AccountId = account_id.as_ref().parse()?;
        self.send_actions(account_id, vec![Action::delete_key(public_key)])
            .await
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Send a transaction with the given actions.
    async fn send_actions(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<FinalExecutionOutcome, Error> {
        let signer = self.signer.as_ref().ok_or(Error::NoSigner)?;

        let signer_id = signer
            .account_id()
            .or(self.default_account.as_ref())
            .ok_or(Error::NoSignerAccount)?
            .clone();

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

        // Build and sign transaction
        let tx = Transaction::new(
            signer_id,
            signer.public_key().clone(),
            access_key.nonce + 1,
            receiver_id,
            block.header.hash,
            actions,
        );

        let signature = signer.sign(tx.get_hash().as_bytes())?;
        let signed_tx = crate::types::SignedTransaction {
            transaction: tx,
            signature,
        };

        // Send transaction
        let outcome = self
            .rpc
            .send_tx(&signed_tx, TxExecutionStatus::ExecutedOptimistic)
            .await?;

        if outcome.is_failure() {
            return Err(Error::TransactionFailed(
                outcome.failure_message().unwrap_or_default(),
            ));
        }

        Ok(outcome)
    }
}

impl std::fmt::Debug for Near {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Near")
            .field("rpc", &self.rpc)
            .field("has_signer", &self.signer.is_some())
            .field("default_account", &self.default_account)
            .finish()
    }
}

/// Builder for creating a [`Near`] client.
pub struct NearBuilder {
    rpc_url: String,
    signer: Option<Arc<dyn Signer>>,
    default_account: Option<AccountId>,
    retry_config: RetryConfig,
}

impl NearBuilder {
    /// Create a new builder with the given RPC URL.
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            signer: None,
            default_account: None,
            retry_config: RetryConfig::default(),
        }
    }

    /// Set the signer for transactions.
    pub fn signer(mut self, signer: impl Signer + 'static) -> Self {
        self.signer = Some(Arc::new(signer));
        self
    }

    /// Set the default account ID for transactions.
    pub fn default_account(mut self, account_id: impl AsRef<str>) -> Self {
        if let Ok(id) = account_id.as_ref().parse() {
            self.default_account = Some(id);
        }
        self
    }

    /// Set the retry configuration.
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Build the client.
    pub fn build(self) -> Near {
        Near {
            rpc: Arc::new(RpcClient::with_retry_config(self.rpc_url, self.retry_config)),
            signer: self.signer,
            default_account: self.default_account,
        }
    }
}

impl From<NearBuilder> for Near {
    fn from(builder: NearBuilder) -> Self {
        builder.build()
    }
}
