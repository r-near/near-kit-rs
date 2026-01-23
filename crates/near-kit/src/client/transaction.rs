//! Transaction builder for fluent multi-action transactions.
//!
//! Allows chaining multiple actions (transfers, function calls, account creation, etc.)
//! into a single atomic transaction. All actions either succeed together or fail together.
//!
//! # Example
//!
//! ```rust,no_run
//! # use near_kit::*;
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
//!     .transfer(NearToken::near(5))
//!     .add_full_access_key(new_public_key)
//!     .deploy(wasm_code)
//!     .call("init")
//!         .args(serde_json::json!({ "owner": "alice.testnet" }))
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::error::{Error, RpcError};
use crate::types::{
    AccountId, Action, BlockReference, CryptoHash, DelegateAction, FinalExecutionOutcome, Finality,
    Gas, IntoGas, IntoNearToken, NearToken, NonDelegateAction, PublicKey, SignedDelegateAction,
    Transaction, TxExecutionStatus,
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
// Delegate Action Types
// ============================================================================

/// Options for creating a delegate action (meta-transaction).
#[derive(Clone, Debug, Default)]
pub struct DelegateOptions {
    /// Explicit block height at which the delegate action expires.
    /// If omitted, uses the current block height plus `block_height_offset`.
    pub max_block_height: Option<u64>,

    /// Number of blocks after the current height when the delegate action should expire.
    /// Defaults to 200 blocks if neither this nor `max_block_height` is provided.
    pub block_height_offset: Option<u64>,

    /// Override nonce to use for the delegate action. If omitted, fetches
    /// from the access key and uses nonce + 1.
    pub nonce: Option<u64>,
}

impl DelegateOptions {
    /// Create options with a specific block height offset.
    pub fn with_offset(offset: u64) -> Self {
        Self {
            block_height_offset: Some(offset),
            ..Default::default()
        }
    }

    /// Create options with a specific max block height.
    pub fn with_max_height(height: u64) -> Self {
        Self {
            max_block_height: Some(height),
            ..Default::default()
        }
    }
}

/// Result of creating a delegate action.
///
/// Contains the signed delegate action plus a pre-encoded payload for transport.
#[derive(Clone, Debug)]
pub struct DelegateResult {
    /// The fully signed delegate action.
    pub signed_delegate_action: SignedDelegateAction,
    /// Base64-encoded payload for HTTP/JSON transport.
    pub payload: String,
}

impl DelegateResult {
    /// Get the raw bytes of the signed delegate action.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.signed_delegate_action.to_bytes()
    }

    /// Get the sender account ID.
    pub fn sender_id(&self) -> &AccountId {
        self.signed_delegate_action.sender_id()
    }

    /// Get the receiver account ID.
    pub fn receiver_id(&self) -> &AccountId {
        self.signed_delegate_action.receiver_id()
    }
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
/// # use near_kit::*;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///     .credentials("ed25519:...", "alice.testnet")?
///     .build();
///
/// // Single action
/// near.transaction("bob.testnet")
///     .transfer(NearToken::near(1))
///     .send()
///     .await?;
///
/// // Multiple actions (atomic)
/// let key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
/// near.transaction("new.alice.testnet")
///     .create_account()
///     .transfer(NearToken::near(5))
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
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("bob.testnet")
    ///     .transfer(NearToken::near(1))
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
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("contract.testnet")
    ///     .call("set_greeting")
    ///         .args(serde_json::json!({ "greeting": "Hello" }))
    ///         .gas(Gas::tgas(10))
    ///         .deposit(NearToken::ZERO)
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

    /// Add a signed delegate action to this transaction (for relayers).
    ///
    /// This is used by relayers to wrap a user's signed delegate action
    /// and submit it to the blockchain, paying for the gas on behalf of the user.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(relayer: Near, payload: &str) -> Result<(), near_kit::Error> {
    /// // Relayer receives base64 payload from user
    /// let signed_delegate = SignedDelegateAction::from_base64(payload)?;
    ///
    /// // Relayer submits it, paying the gas
    /// let result = relayer
    ///     .transaction(signed_delegate.sender_id().as_str())
    ///     .signed_delegate_action(signed_delegate)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn signed_delegate_action(mut self, signed_delegate: SignedDelegateAction) -> Self {
        // Set receiver_id to the sender of the delegate action (the original user)
        self.receiver_id = signed_delegate.sender_id().clone();
        self.actions.push(Action::delegate(signed_delegate));
        self
    }

    // ========================================================================
    // Meta-transactions (Delegate Actions)
    // ========================================================================

    /// Build and sign a delegate action for meta-transactions (NEP-366).
    ///
    /// This allows the user to sign a set of actions off-chain, which can then
    /// be submitted by a relayer who pays the gas fees. The user's signature
    /// authorizes the actions, but they don't need to hold NEAR for gas.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// // User builds and signs a delegate action
    /// let result = near
    ///     .transaction("contract.testnet")
    ///     .call("add_message")
    ///         .args(serde_json::json!({ "text": "Hello!" }))
    ///         .gas(Gas::tgas(30))
    ///     .delegate(Default::default())
    ///     .await?;
    ///
    /// // Send payload to relayer via HTTP
    /// println!("Payload to send: {}", result.payload);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delegate(self, options: DelegateOptions) -> Result<DelegateResult, Error> {
        if self.actions.is_empty() {
            return Err(Error::InvalidTransaction(
                "Delegate action requires at least one action".to_string(),
            ));
        }

        // Verify no nested delegates
        for action in &self.actions {
            if matches!(action, Action::Delegate(_)) {
                return Err(Error::InvalidTransaction(
                    "Delegate actions cannot contain nested signed delegate actions".to_string(),
                ));
            }
        }

        // Get the signer
        let signer = self
            .signer_override
            .as_ref()
            .or(self.signer.as_ref())
            .ok_or(Error::NoSigner)?;

        let sender_id = signer.account_id().clone();
        let public_key = signer.public_key().clone();

        // Get nonce
        let nonce = if let Some(n) = options.nonce {
            n
        } else {
            let access_key = self
                .rpc
                .view_access_key(
                    &sender_id,
                    &public_key,
                    BlockReference::Finality(Finality::Optimistic),
                )
                .await?;
            access_key.nonce + 1
        };

        // Get max block height
        let max_block_height = if let Some(h) = options.max_block_height {
            h
        } else {
            let status = self.rpc.status().await?;
            let offset = options.block_height_offset.unwrap_or(200);
            status.sync_info.latest_block_height + offset
        };

        // Convert actions to NonDelegateAction
        let delegate_actions: Vec<NonDelegateAction> = self
            .actions
            .into_iter()
            .filter_map(NonDelegateAction::from_action)
            .collect();

        // Create delegate action
        let delegate_action = DelegateAction {
            sender_id,
            receiver_id: self.receiver_id,
            actions: delegate_actions,
            nonce,
            max_block_height,
            public_key: public_key.clone(),
        };

        // Sign the delegate action
        let hash = delegate_action.get_hash();
        let (signature, _) = signer.sign(hash.as_bytes()).await?;

        // Create signed delegate action
        let signed_delegate_action = delegate_action.sign(signature);
        let payload = signed_delegate_action.to_base64();

        Ok(DelegateResult {
            signed_delegate_action,
            payload,
        })
    }

    // ========================================================================
    // Global Contract Actions
    // ========================================================================

    /// Publish a contract to the global registry.
    ///
    /// Global contracts are deployed once and can be referenced by multiple accounts,
    /// saving storage costs. Two modes are available:
    ///
    /// - `by_hash = false` (default): Contract is identified by the signer's account ID.
    ///   The signer can update the contract later, and all users will automatically
    ///   use the updated version.
    ///
    /// - `by_hash = true`: Contract is identified by its code hash. This creates
    ///   an immutable contract that cannot be updated.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let wasm_code = std::fs::read("contract.wasm")?;
    ///
    /// // Publish updatable contract (identified by your account)
    /// near.transaction("alice.testnet")
    ///     .publish_contract(wasm_code.clone(), false)
    ///     .send()
    ///     .await?;
    ///
    /// // Publish immutable contract (identified by its hash)
    /// near.transaction("alice.testnet")
    ///     .publish_contract(wasm_code, true)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn publish_contract(mut self, code: impl Into<Vec<u8>>, by_hash: bool) -> Self {
        self.actions
            .push(Action::publish_contract(code.into(), by_hash));
        self
    }

    /// Deploy a contract from the global registry by code hash.
    ///
    /// References a previously published immutable contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// near.transaction("alice.testnet")
    ///     .deploy_from_hash(code_hash)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn deploy_from_hash(mut self, code_hash: CryptoHash) -> Self {
        self.actions.push(Action::deploy_from_hash(code_hash));
        self
    }

    /// Deploy a contract from the global registry by publisher account.
    ///
    /// References a contract published by the given account.
    /// The contract can be updated by the publisher.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("alice.testnet")
    ///     .deploy_from_publisher("contract-publisher.near")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn deploy_from_publisher(mut self, publisher_id: impl AsRef<str>) -> Self {
        let publisher_id: AccountId = publisher_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(publisher_id.as_ref()));
        self.actions.push(Action::deploy_from_account(publisher_id));
        self
    }

    /// Create a NEP-616 deterministic state init action with code hash reference.
    ///
    /// The account ID is derived from the state init data:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// near.transaction("alice.testnet")
    ///     .state_init_by_hash(code_hash, Default::default(), NearToken::near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn state_init_by_hash(
        mut self,
        code_hash: CryptoHash,
        data: BTreeMap<Vec<u8>, Vec<u8>>,
        deposit: impl IntoNearToken,
    ) -> Self {
        let deposit = deposit.into_near_token().unwrap_or(NearToken::ZERO);
        self.actions
            .push(Action::state_init_by_hash(code_hash, data, deposit));
        self
    }

    /// Create a NEP-616 deterministic state init action with publisher account reference.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("alice.testnet")
    ///     .state_init_by_publisher("contract-publisher.near", Default::default(), NearToken::near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn state_init_by_publisher(
        mut self,
        publisher_id: impl AsRef<str>,
        data: BTreeMap<Vec<u8>, Vec<u8>>,
        deposit: impl IntoNearToken,
    ) -> Self {
        let publisher_id: AccountId = publisher_id
            .as_ref()
            .parse()
            .unwrap_or_else(|_| AccountId::new_unchecked(publisher_id.as_ref()));
        let deposit = deposit.into_near_token().unwrap_or(NearToken::ZERO);
        self.actions
            .push(Action::state_init_by_account(publisher_id, data, deposit));
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

    /// Set gas limit.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("contract.testnet")
    ///     .call("method")
    ///         .gas(Gas::tgas(50))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        if let Ok(g) = gas.into_gas() {
            self.gas = g;
        }
        self
    }

    /// Set attached deposit.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// near.transaction("contract.testnet")
    ///     .call("method")
    ///         .deposit(NearToken::near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Publish a contract to the global registry.
    pub fn publish_contract(self, code: impl Into<Vec<u8>>, by_hash: bool) -> TransactionBuilder {
        self.finish().publish_contract(code, by_hash)
    }

    /// Deploy a contract from the global registry by code hash.
    pub fn deploy_from_hash(self, code_hash: CryptoHash) -> TransactionBuilder {
        self.finish().deploy_from_hash(code_hash)
    }

    /// Deploy a contract from the global registry by publisher account.
    pub fn deploy_from_publisher(self, publisher_id: impl AsRef<str>) -> TransactionBuilder {
        self.finish().deploy_from_publisher(publisher_id)
    }

    /// Create a NEP-616 deterministic state init action with code hash reference.
    pub fn state_init_by_hash(
        self,
        code_hash: CryptoHash,
        data: BTreeMap<Vec<u8>, Vec<u8>>,
        deposit: impl IntoNearToken,
    ) -> TransactionBuilder {
        self.finish().state_init_by_hash(code_hash, data, deposit)
    }

    /// Create a NEP-616 deterministic state init action with publisher account reference.
    pub fn state_init_by_publisher(
        self,
        publisher_id: impl AsRef<str>,
        data: BTreeMap<Vec<u8>, Vec<u8>>,
        deposit: impl IntoNearToken,
    ) -> TransactionBuilder {
        self.finish()
            .state_init_by_publisher(publisher_id, data, deposit)
    }

    /// Override the signer.
    pub fn sign_with(self, signer: impl Signer + 'static) -> TransactionBuilder {
        self.finish().sign_with(signer)
    }

    /// Set the execution wait level.
    pub fn wait_until(self, status: TxExecutionStatus) -> TransactionBuilder {
        self.finish().wait_until(status)
    }

    /// Build and sign a delegate action for meta-transactions (NEP-366).
    ///
    /// This finishes the current function call and then creates a delegate action.
    pub async fn delegate(self, options: DelegateOptions) -> Result<DelegateResult, crate::Error> {
        self.finish().delegate(options).await
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
                let (signature, _) = signer.sign(tx.get_hash().as_bytes()).await?;
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
