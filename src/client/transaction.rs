//! Transaction builder for fluent multi-action transactions.
//!
//! Allows chaining multiple actions (transfers, function calls, account creation, etc.)
//! into a single atomic transaction. All actions either succeed together or fail together.
//!
//! # Example
//!
//! ```rust,no_run
//! # use near_kit::prelude::*;
//! # async fn example() -> Result<(), near_kit::Error> {
//! let near = Near::testnet()
//!     .credentials("ed25519:...", "alice.testnet")?
//!     .build();
//!
//! // Create a new sub-account with funding and a key
//! let new_public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
//! let wasm_code = std::fs::read("contract.wasm").expect("failed to read wasm");
//! near.transaction("new.alice.testnet")
//!     .create_account()
//!     .transfer("5 NEAR")
//!     .add_full_access_key(new_public_key)
//!     .deploy(wasm_code)
//!     .call("init")
//!         .args(serde_json::json!({ "owner": "alice.testnet" }))
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::error::{Error, RpcError};
use crate::types::{
    AccountId, Action, BlockReference, FinalExecutionOutcome, Finality, Gas, IntoGas,
    IntoNearToken, NearToken, PublicKey, Transaction, TxExecutionStatus,
};

use super::nonce_manager::NonceManager;
use super::rpc::RpcClient;
use super::signer::Signer;

/// Global nonce manager shared across all TransactionBuilder instances.
/// This is an implementation detail - not exposed to users.
fn nonce_manager() -> &'static NonceManager {
    static NONCE_MANAGER: OnceLock<NonceManager> = OnceLock::new();
    NONCE_MANAGER.get_or_init(NonceManager::new)
}

// ============================================================================
// TransactionBuilder
// ============================================================================

/// Builder for constructing multi-action transactions.
///
/// Created via [`Near::transaction`]. Supports chaining multiple actions
/// into a single atomic transaction.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::prelude::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///     .credentials("ed25519:...", "alice.testnet")?
///     .build();
///
/// // Single action
/// near.transaction("bob.testnet")
///     .transfer("1 NEAR")
///     .send()
///     .await?;
///
/// // Multiple actions (atomic)
/// let key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
/// near.transaction("new.alice.testnet")
///     .create_account()
///     .transfer("5 NEAR")
///     .add_full_access_key(key)
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct TransactionBuilder {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    receiver_id: AccountId,
    actions: Vec<Action>,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl TransactionBuilder {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        receiver_id: AccountId,
    ) -> Self {
        Self {
            rpc,
            signer,
            receiver_id,
            actions: Vec::new(),
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    // ========================================================================
    // Action methods
    // ========================================================================

    /// Add a create account action.
    ///
    /// Creates a new sub-account. Must be followed by `transfer` and `add_key`
    /// to properly initialize the account.
    pub fn create_account(mut self) -> Self {
        self.actions.push(Action::create_account());
        self
    }

    /// Add a transfer action.
    ///
    /// Transfers NEAR tokens to the receiver account.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("bob.testnet")
    ///     .transfer("1 NEAR")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(mut self, amount: impl IntoNearToken) -> Self {
        let amount = amount.into_near_token().unwrap_or(NearToken::ZERO);
        self.actions.push(Action::transfer(amount));
        self
    }

    /// Add a deploy contract action.
    ///
    /// Deploys WASM code to the receiver account.
    pub fn deploy(mut self, code: impl Into<Vec<u8>>) -> Self {
        self.actions.push(Action::deploy_contract(code.into()));
        self
    }

    /// Add a function call action.
    ///
    /// Returns a [`CallBuilder`] for configuring the call with args, gas, and deposit.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::prelude::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("contract.testnet")
    ///     .call("set_greeting")
    ///         .args(serde_json::json!({ "greeting": "Hello" }))
    ///         .gas("10 Tgas")
    ///         .deposit("0 NEAR")
    ///     .call("another_method")
    ///         .args(serde_json::json!({ "value": 42 }))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(self, method: &str) -> CallBuilder {
        CallBuilder::new(self, method.to_string())
    }

    /// Add a full access key to the account.
    pub fn add_full_access_key(mut self, public_key: PublicKey) -> Self {
        self.actions.push(Action::add_full_access_key(public_key));
        self
    }

    /// Add a function call access key to the account.
    ///
    /// # Arguments
    ///
    /// * `public_key` - The public key to add
    /// * `receiver_id` - The contract this key can call
    /// * `method_names` - Methods this key can call (empty = all methods)
    /// * `allowance` - Maximum amount this key can spend (None = unlimited)
    pub fn add_function_call_key(
        mut self,
        public_key: PublicKey,
        receiver_id: impl AsRef<str>,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        let receiver_id: AccountId = receiver_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(receiver_id.as_ref()));
        self.actions.push(Action::add_function_call_key(
            public_key,
            receiver_id,
            method_names,
            allowance,
        ));
        self
    }

    /// Delete an access key from the account.
    pub fn delete_key(mut self, public_key: PublicKey) -> Self {
        self.actions.push(Action::delete_key(public_key));
        self
    }

    /// Delete the account and transfer remaining balance to beneficiary.
    pub fn delete_account(mut self, beneficiary_id: impl AsRef<str>) -> Self {
        let beneficiary_id: AccountId = beneficiary_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(beneficiary_id.as_ref()));
        self.actions.push(Action::delete_account(beneficiary_id));
        self
    }

    /// Add a stake action.
    pub fn stake(mut self, amount: impl IntoNearToken, public_key: PublicKey) -> Self {
        let amount = amount.into_near_token().unwrap_or(NearToken::ZERO);
        self.actions.push(Action::stake(amount, public_key));
        self
    }

    // ========================================================================
    // Configuration methods
    // ========================================================================

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

    // ========================================================================
    // Execution
    // ========================================================================

    /// Send the transaction.
    ///
    /// This is equivalent to awaiting the builder directly.
    pub fn send(self) -> TransactionSend {
        TransactionSend { builder: self }
    }

    /// Internal method to add an action (used by CallBuilder).
    fn push_action(&mut self, action: Action) {
        self.actions.push(action);
    }
}

// ============================================================================
// CallBuilder
// ============================================================================

/// Builder for configuring a function call within a transaction.
///
/// Created via [`TransactionBuilder::call`]. Allows setting args, gas, and deposit
/// before continuing to chain more actions or sending.
pub struct CallBuilder {
    builder: TransactionBuilder,
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
}

impl CallBuilder {
    fn new(builder: TransactionBuilder, method: String) -> Self {
        Self {
            builder,
            method,
            args: Vec::new(),
            gas: Gas::DEFAULT,
            deposit: NearToken::ZERO,
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

    /// Set gas limit (accepts string like "30 Tgas" or Gas value).
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        if let Ok(g) = gas.into_gas() {
            self.gas = g;
        }
        self
    }

    /// Set attached deposit (accepts string like "1 NEAR" or NearToken value).
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        if let Ok(a) = amount.into_near_token() {
            self.deposit = a;
        }
        self
    }

    /// Finish this call and return to the transaction builder.
    fn finish(self) -> TransactionBuilder {
        let mut builder = self.builder;
        builder.push_action(Action::function_call(
            self.method,
            self.args,
            self.gas,
            self.deposit,
        ));
        builder
    }

    // ========================================================================
    // Chaining methods (delegate to TransactionBuilder after finishing)
    // ========================================================================

    /// Add another function call.
    pub fn call(self, method: &str) -> CallBuilder {
        self.finish().call(method)
    }

    /// Add a create account action.
    pub fn create_account(self) -> TransactionBuilder {
        self.finish().create_account()
    }

    /// Add a transfer action.
    pub fn transfer(self, amount: impl IntoNearToken) -> TransactionBuilder {
        self.finish().transfer(amount)
    }

    /// Add a deploy contract action.
    pub fn deploy(self, code: impl Into<Vec<u8>>) -> TransactionBuilder {
        self.finish().deploy(code)
    }

    /// Add a full access key.
    pub fn add_full_access_key(self, public_key: PublicKey) -> TransactionBuilder {
        self.finish().add_full_access_key(public_key)
    }

    /// Add a function call access key.
    pub fn add_function_call_key(
        self,
        public_key: PublicKey,
        receiver_id: impl AsRef<str>,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> TransactionBuilder {
        self.finish()
            .add_function_call_key(public_key, receiver_id, method_names, allowance)
    }

    /// Delete an access key.
    pub fn delete_key(self, public_key: PublicKey) -> TransactionBuilder {
        self.finish().delete_key(public_key)
    }

    /// Delete the account.
    pub fn delete_account(self, beneficiary_id: impl AsRef<str>) -> TransactionBuilder {
        self.finish().delete_account(beneficiary_id)
    }

    /// Add a stake action.
    pub fn stake(self, amount: impl IntoNearToken, public_key: PublicKey) -> TransactionBuilder {
        self.finish().stake(amount, public_key)
    }

    /// Override the signer.
    pub fn sign_with(self, signer: impl Signer + 'static) -> TransactionBuilder {
        self.finish().sign_with(signer)
    }

    /// Set the execution wait level.
    pub fn wait_until(self, status: TxExecutionStatus) -> TransactionBuilder {
        self.finish().wait_until(status)
    }

    /// Send the transaction.
    pub fn send(self) -> TransactionSend {
        self.finish().send()
    }
}

impl IntoFuture for CallBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        self.send().into_future()
    }
}

// ============================================================================
// TransactionSend
// ============================================================================

/// Future for sending a transaction.
pub struct TransactionSend {
    builder: TransactionBuilder,
}

impl TransactionSend {
    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.builder.wait_until = status;
        self
    }
}

impl IntoFuture for TransactionSend {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let builder = self.builder;

            if builder.actions.is_empty() {
                return Err(Error::InvalidTransaction(
                    "Transaction must have at least one action".to_string(),
                ));
            }

            let signer = builder
                .signer_override
                .as_ref()
                .or(builder.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();
            let public_key = signer.public_key().clone();
            let public_key_str = public_key.to_string();

            // Retry loop for InvalidNonceError
            const MAX_NONCE_RETRIES: u32 = 3;
            let mut last_error: Option<Error> = None;

            for attempt in 0..MAX_NONCE_RETRIES {
                // Get nonce from manager (fetches from blockchain on first call, then increments locally)
                let rpc = builder.rpc.clone();
                let signer_id_clone = signer_id.clone();
                let public_key_clone = public_key.clone();

                let nonce = if attempt > 0 {
                    // Invalidate on retry to get fresh nonce
                    nonce_manager().invalidate(signer_id.as_ref(), &public_key_str);
                    nonce_manager()
                        .get_next_nonce(signer_id.as_ref(), &public_key_str, || async {
                            let access_key = rpc
                                .view_access_key(
                                    &signer_id_clone,
                                    &public_key_clone,
                                    BlockReference::Finality(Finality::Optimistic),
                                )
                                .await?;
                            Ok(access_key.nonce)
                        })
                        .await?
                } else {
                    nonce_manager()
                        .get_next_nonce(signer_id.as_ref(), &public_key_str, || async {
                            let access_key = rpc
                                .view_access_key(
                                    &signer_id_clone,
                                    &public_key_clone,
                                    BlockReference::Finality(Finality::Optimistic),
                                )
                                .await?;
                            Ok(access_key.nonce)
                        })
                        .await?
                };

                // Get recent block hash (use finalized for stability)
                let block = builder
                    .rpc
                    .block(BlockReference::Finality(Finality::Final))
                    .await?;

                // Build transaction
                let tx = Transaction::new(
                    signer_id.clone(),
                    public_key.clone(),
                    nonce,
                    builder.receiver_id.clone(),
                    block.header.hash,
                    builder.actions.clone(),
                );

                // Sign
                let signature = signer.sign(tx.get_hash().as_bytes())?;
                let signed_tx = crate::types::SignedTransaction {
                    transaction: tx,
                    signature,
                };

                // Send
                match builder.rpc.send_tx(&signed_tx, builder.wait_until).await {
                    Ok(outcome) => {
                        if outcome.is_failure() {
                            return Err(Error::TransactionFailed(
                                outcome.failure_message().unwrap_or_default(),
                            ));
                        }
                        return Ok(outcome);
                    }
                    Err(RpcError::InvalidNonce { .. }) if attempt < MAX_NONCE_RETRIES - 1 => {
                        // Retry with fresh nonce
                        last_error = Some(Error::Rpc(RpcError::InvalidNonce {
                            tx_nonce: nonce,
                            ak_nonce: 0,
                        }));
                        continue;
                    }
                    Err(e) => return Err(Error::Rpc(e)),
                }
            }

            Err(last_error.unwrap_or_else(|| {
                Error::InvalidTransaction("Unknown error during transaction send".to_string())
            }))
        })
    }
}

impl IntoFuture for TransactionBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        self.send().into_future()
    }
}
