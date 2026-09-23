//! Transaction builder for fluent multi-action transactions.
//!
//! Allows chaining multiple actions (transfers, function calls, account creation, etc.)
//! into a single atomic transaction. All actions either succeed together or fail together.
//!
//! # Example
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # use near_kit::signer::PublicKey;
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
//!     .transfer(NearToken::from_near(5))
//!     .add_full_access_key(new_public_key)
//!     .deploy(wasm_code)
//!     .call("init")
//!         .args(serde_json::json!({ "owner": "alice.testnet" }))
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::convert::Infallible;
use std::fmt;
use std::future::IntoFuture;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

use crate::trace::{self, Instrument};

use crate::error::{Error, RpcError};
use crate::transaction::FunctionCall;
use crate::types::{
    AccountId, Action, BlockReference, CryptoHash, DelegateAction, FinalExecutionOutcome, Finality,
    IntoGas, IntoNearToken, NearToken, NonDelegateAction, PublicKey, PublishMode,
    SignedDelegateAction, SignedTransaction, StateInit, Transaction, TryIntoAccountId,
    TryIntoGlobalContractId, UseGlobalContractAction, WaitLevel,
};

use super::nonce_manager::NonceManager;
use super::rpc::RpcClient;
use super::signer::{Signer, SigningKey};

/// Global nonce manager shared across all TransactionBuilder instances.
/// This is an implementation detail - not exposed to users.
fn nonce_manager() -> &'static NonceManager {
    static NONCE_MANAGER: OnceLock<NonceManager> = OnceLock::new();
    NONCE_MANAGER.get_or_init(NonceManager::new)
}

impl From<Infallible> for Error {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

/// Produce a comma-separated summary of action types for tracing spans.
///
/// Function calls include the method name, e.g. `"create_account,transfer,function_call(init)"`.
#[cfg(feature = "tracing")]
fn actions_summary(actions: &[Action]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, a) in actions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match a {
            Action::CreateAccount(_) => out.push_str("create_account"),
            Action::DeployContract(_) => out.push_str("deploy_contract"),
            Action::FunctionCall(fc) => write!(out, "function_call({})", fc.method_name).unwrap(),
            Action::Transfer(_) => out.push_str("transfer"),
            Action::Stake(_) => out.push_str("stake"),
            Action::AddKey(_) => out.push_str("add_key"),
            Action::DeleteKey(_) => out.push_str("delete_key"),
            Action::DeleteAccount(_) => out.push_str("delete_account"),
            Action::Delegate(_) => out.push_str("delegate"),
            Action::DeployGlobalContract(_) => out.push_str("deploy_global_contract"),
            Action::UseGlobalContract(_) => out.push_str("use_global_contract"),
            Action::DeterministicStateInit(_) => out.push_str("deterministic_state_init"),
            Action::TransferToGasKey(_) => out.push_str("transfer_to_gas_key"),
            Action::WithdrawFromGasKey(_) => out.push_str("withdraw_from_gas_key"),
            Action::DelegateV2(_) => out.push_str("delegate_v2"),
        }
    }
    out
}

/// Record function-call span fields from the action list.
///
/// When the actions contain exactly one function call, records `method`, `gas`,
/// and `deposit` on the current span. Multiple function calls emit a debug event
/// per call instead, since span fields can't repeat.
#[cfg(feature = "tracing")]
fn record_function_call_fields(actions: &[Action]) {
    let function_calls: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            Action::FunctionCall(fc) => Some(fc),
            _ => None,
        })
        .collect();

    match function_calls.as_slice() {
        [fc] => {
            let span = trace::Span::current();
            span.record("method", fc.method_name.as_str());
            span.record("gas", trace::field::display(&fc.gas));
            span.record("deposit", trace::field::display(&fc.deposit));
        }
        multiple if !multiple.is_empty() => {
            for fc in multiple {
                trace::debug!(
                    method = %fc.method_name,
                    gas = %fc.gas,
                    deposit = %fc.deposit,
                    "function_call action"
                );
            }
        }
        _ => {}
    }
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
/// Created via [`crate::Near::transaction`]. Supports chaining multiple actions
/// into a single atomic transaction.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # use near_kit::signer::PublicKey;
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet()
///     .credentials("ed25519:...", "alice.testnet")?
///     .build();
///
/// // Single action
/// near.transaction("bob.testnet")
///     .transfer(NearToken::from_near(1))
///     .send()
///     .await?;
///
/// // Multiple actions (atomic)
/// let key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
/// near.transaction("new.alice.testnet")
///     .create_account()
///     .transfer(NearToken::from_near(5))
///     .add_full_access_key(key)
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct TransactionBuilder {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    receiver_id: Result<AccountId, Error>,
    actions: Vec<Action>,
    construction_error: Option<Error>,
    signer_override: Option<Arc<dyn Signer>>,
    max_nonce_retries: u32,
}

impl fmt::Debug for TransactionBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransactionBuilder")
            .field(
                "signer_id",
                &self
                    .signer_override
                    .as_ref()
                    .or(self.signer.as_ref())
                    .map(|s| s.account_id()),
            )
            .field("receiver_id", &self.receiver_id)
            .field("action_count", &self.actions.len())
            .field("construction_error", &self.construction_error)
            .field("max_nonce_retries", &self.max_nonce_retries)
            .finish()
    }
}

/// A transaction whose receiver and actions have passed builder validation.
///
/// Keeping this stage private lets every terminal path share input validation,
/// signer selection, online nonce resolution, and signing without exposing
/// partially prepared state in the public API.
struct PreparedTransaction {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    receiver_id: AccountId,
    actions: Vec<Action>,
    max_nonce_retries: u32,
}

impl PreparedTransaction {
    /// Peek at the configured signer without claiming a rotating key.
    fn peek_signer(&self) -> Result<(AccountId, PublicKey), Error> {
        let signer = self.signer.as_ref().ok_or(Error::NoSigner)?;
        Ok((signer.account_id().clone(), signer.public_key()))
    }

    /// Claim one signing key for the logical operation.
    fn claim_signer(&self) -> Result<(AccountId, SigningKey), Error> {
        let signer = self.signer.as_ref().ok_or(Error::NoSigner)?;
        Ok((signer.account_id().clone(), signer.key()))
    }

    /// Resolve the next nonce and block hash for one online attempt.
    async fn online_context(
        &self,
        signer_id: &AccountId,
        public_key: &PublicKey,
        nonce_floor: Option<u64>,
    ) -> Result<(u64, CryptoHash), Error> {
        let access_key = self
            .rpc
            .view_access_key(
                signer_id,
                public_key,
                BlockReference::Finality(Finality::Final),
            )
            .await?;
        let nonce = nonce_manager().next(
            self.rpc.url().to_string(),
            signer_id.clone(),
            public_key.clone(),
            nonce_floor.unwrap_or(access_key.nonce),
        );
        Ok((nonce, access_key.block_hash))
    }

    fn transaction(
        &self,
        signer_id: AccountId,
        public_key: PublicKey,
        nonce: u64,
        block_hash: CryptoHash,
    ) -> Transaction {
        Transaction::new(
            signer_id,
            public_key,
            nonce,
            self.receiver_id.clone(),
            block_hash,
            self.actions.clone(),
        )
    }

    fn into_transaction(
        self,
        signer_id: AccountId,
        public_key: PublicKey,
        nonce: u64,
        block_hash: CryptoHash,
    ) -> Transaction {
        Transaction::new(
            signer_id,
            public_key,
            nonce,
            self.receiver_id,
            block_hash,
            self.actions,
        )
    }

    async fn sign_transaction(
        transaction: Transaction,
        key: &SigningKey,
    ) -> Result<(SignedTransaction, CryptoHash), Error> {
        let tx_hash = transaction.get_hash();
        let signature = key.sign(tx_hash.as_bytes()).await?;
        Ok((
            SignedTransaction {
                transaction,
                signature,
            },
            tx_hash,
        ))
    }
}

impl TransactionBuilder {
    pub(crate) fn new_fallible(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        receiver_id: Result<AccountId, Error>,
        max_nonce_retries: u32,
    ) -> Self {
        Self {
            rpc,
            signer,
            receiver_id,
            actions: Vec::new(),
            construction_error: None,
            signer_override: None,
            max_nonce_retries,
        }
    }

    fn defer_error(&mut self, error: Error) {
        if self.construction_error.is_none() {
            self.construction_error = Some(error);
        }
    }

    fn prepare(self, empty_error: &'static str) -> Result<PreparedTransaction, Error> {
        let receiver_id = self.receiver_id?;
        if let Some(error) = self.construction_error {
            return Err(error);
        }
        if self.actions.is_empty() {
            return Err(Error::InvalidTransaction(empty_error.to_string()));
        }

        Ok(PreparedTransaction {
            rpc: self.rpc,
            signer: self.signer_override.or(self.signer),
            receiver_id,
            actions: self.actions,
            max_nonce_retries: self.max_nonce_retries,
        })
    }

    pub(crate) fn with_validation(mut self, validation: Result<(), Error>) -> Self {
        if let Err(error) = validation {
            self.defer_error(error);
        }
        self
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
    ///     .transfer(NearToken::from_near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Parsing failures are retained and returned when the transaction is sent,
    /// built, signed, or delegated.
    pub fn transfer(mut self, amount: impl IntoNearToken) -> Self {
        match amount.into_near_token() {
            Ok(amount) => self.actions.push(Action::transfer(amount)),
            Err(error) => self.defer_error(error.into()),
        }
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
    ///         .gas(Gas::from_tgas(10))
    ///         .deposit(NearToken::ZERO)
    ///     .finish()
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
        receiver_id: impl TryIntoAccountId,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        match receiver_id.try_into_account_id() {
            Ok(receiver_id) => self.actions.push(Action::add_function_call_key(
                public_key,
                receiver_id,
                method_names,
                allowance,
            )),
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Delete an access key from the account.
    pub fn delete_key(mut self, public_key: PublicKey) -> Self {
        self.actions.push(Action::delete_key(public_key));
        self
    }

    /// Delete the account and transfer remaining balance to beneficiary.
    pub fn delete_account(mut self, beneficiary_id: impl TryIntoAccountId) -> Self {
        match beneficiary_id.try_into_account_id() {
            Ok(beneficiary_id) => self.actions.push(Action::delete_account(beneficiary_id)),
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Add a stake action.
    ///
    /// Parsing failures are retained and returned when the transaction is sent,
    /// built, signed, or delegated.
    pub fn stake(mut self, amount: impl IntoNearToken, public_key: PublicKey) -> Self {
        match amount.into_near_token() {
            Ok(amount) => self.actions.push(Action::stake(amount, public_key)),
            Err(error) => self.defer_error(error.into()),
        }
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
    /// # use near_kit::protocol::SignedDelegateAction;
    /// # async fn example(relayer: Near, payload: &str) -> Result<(), near_kit::Error> {
    /// // Relayer receives base64 payload from user
    /// let signed_delegate = SignedDelegateAction::from_base64(payload)?;
    ///
    /// // Relayer submits it, paying the gas
    /// let result = relayer
    ///     .transaction(signed_delegate.sender_id())
    ///     .signed_delegate_action(signed_delegate)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn signed_delegate_action(mut self, signed_delegate: SignedDelegateAction) -> Self {
        // Set receiver_id to the sender of the delegate action (the original user)
        self.receiver_id = self
            .receiver_id
            .map(|_| signed_delegate.sender_id().clone());
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
    ///         .gas(Gas::from_tgas(30))
    ///     .finish()
    ///     .delegate(Default::default())
    ///     .await?;
    ///
    /// // Send payload to relayer via HTTP
    /// println!("Payload to send: {}", result.payload);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delegate(self, options: DelegateOptions) -> Result<DelegateResult, Error> {
        let prepared = self.prepare("Delegate action requires at least one action")?;

        // Verify no nested delegates (of any version)
        for action in &prepared.actions {
            if action.is_delegate() {
                return Err(Error::InvalidTransaction(
                    "Delegate actions cannot contain nested signed delegate actions".to_string(),
                ));
            }
        }

        let (sender_id, key) = prepared.claim_signer()?;
        let public_key = key.public_key().clone();

        // Get nonce
        let nonce = if let Some(n) = options.nonce {
            n
        } else {
            let access_key = prepared
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
            let status = prepared.rpc.status().await?;
            let offset = options.block_height_offset.unwrap_or(200);
            status.sync_info.latest_block_height + offset
        };

        let PreparedTransaction {
            receiver_id,
            actions,
            ..
        } = prepared;

        // Convert actions to NonDelegateAction
        let delegate_actions: Vec<NonDelegateAction> = actions
            .into_iter()
            .filter_map(NonDelegateAction::from_action)
            .collect();

        // Create delegate action
        let delegate_action = DelegateAction {
            sender_id,
            receiver_id,
            actions: delegate_actions,
            nonce,
            max_block_height,
            public_key: public_key.clone(),
        };

        // Sign the delegate action
        let hash = delegate_action.get_hash();
        let signature = key.sign(hash.as_bytes()).await?;

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
    /// saving storage costs. Two modes are available via [`PublishMode`]:
    ///
    /// - [`PublishMode::Updatable`]: the contract is identified by the publisher's
    ///   account and can be updated by publishing new code from the same account.
    /// - [`PublishMode::Immutable`]: the contract is identified by its code hash and
    ///   cannot be updated once published.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::PublishMode;
    /// # async fn example(near: Near) -> Result<(), Box<dyn std::error::Error>> {
    /// let wasm_code = std::fs::read("contract.wasm")?;
    ///
    /// // Publish updatable contract (identified by your account)
    /// near.transaction("alice.testnet")
    ///     .publish(wasm_code.clone(), PublishMode::Updatable)
    ///     .send()
    ///     .await?;
    ///
    /// // Publish immutable contract (identified by its hash)
    /// near.transaction("alice.testnet")
    ///     .publish(wasm_code, PublishMode::Immutable)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn publish(mut self, code: impl Into<Vec<u8>>, mode: PublishMode) -> Self {
        self.actions.push(Action::publish(code.into(), mode));
        self
    }

    /// Deploy a contract from the global registry.
    ///
    /// Accepts any [`TryIntoGlobalContractId`] (such as a [`CryptoHash`] or an account ID
    /// string/[`AccountId`]) to reference a previously published contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// near.transaction("alice.testnet")
    ///     .deploy_from(code_hash)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn deploy_from(mut self, contract_ref: impl TryIntoGlobalContractId) -> Self {
        match contract_ref.try_into_identifier() {
            Ok(identifier) => {
                self.actions
                    .push(Action::UseGlobalContract(UseGlobalContractAction {
                        contract_identifier: identifier,
                    }));
            }
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Create a NEP-616 deterministic state init action.
    ///
    /// The receiver_id is automatically set to the deterministically derived account ID:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::{StateInit, StateInitExt};
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// let si = StateInit::by_hash(code_hash, Default::default());
    /// let outcome = near.transaction("alice.testnet")
    ///     .state_init(si, NearToken::from_near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Parsing failures are retained and returned when the transaction is sent,
    /// built, signed, or delegated.
    pub fn state_init(mut self, state_init: StateInit, deposit: impl IntoNearToken) -> Self {
        match deposit.into_near_token() {
            Ok(deposit) => {
                self.receiver_id = self.receiver_id.map(|_| state_init.derive_account_id());
                self.actions.push(Action::state_init(state_init, deposit));
            }
            Err(error) => self.defer_error(error.into()),
        }
        self
    }

    /// Add a pre-built action to the transaction.
    ///
    /// This is the most flexible way to add actions, since it accepts any
    /// [`Action`] variant directly. It's especially useful when you want to
    /// build function call actions independently and attach them later, or
    /// when working with action types that don't have dedicated builder
    /// methods.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let action = Action::function_call(
    ///     "transfer",
    ///     serde_json::to_vec(&serde_json::json!({ "receiver": "bob.testnet" }))?,
    ///     Gas::from_tgas(30),
    ///     NearToken::ZERO,
    /// );
    ///
    /// near.transaction("contract.testnet")
    ///     .add_action(action)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_action<A>(mut self, action: A) -> Self
    where
        A: TryInto<Action>,
        Error: From<A::Error>,
    {
        match action.try_into() {
            Ok(action) => self.actions.push(action),
            Err(error) => self.defer_error(error.into()),
        }
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

    /// Set the execution wait level and prepare to send.
    ///
    /// This is a shorthand for `.send().wait_until::<W>()`.
    /// The return type changes based on the wait level — see [`TransactionSend::wait_until`].
    pub fn wait_until<W: crate::types::WaitLevel>(self) -> TransactionSend<W> {
        self.send().wait_until::<W>()
    }

    /// Override the number of nonce retries for this transaction on `InvalidNonce`
    /// errors. `0` means no retries (send once), `1` means one retry, etc.
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.max_nonce_retries = retries;
        self
    }

    // ========================================================================
    // Execution
    // ========================================================================

    /// Build the unsigned transaction without signing.
    ///
    /// Resolves the nonce and block hash from the network, then returns the
    /// unsigned [`Transaction`]. Use this for external signing workflows
    /// (hardware wallets, MPC, HSM) where you need the transaction hash
    /// before signing.
    ///
    /// The returned [`Transaction`] provides [`get_hash()`](Transaction::get_hash)
    /// for the bytes to sign, and [`complete()`](Transaction::complete) to attach
    /// an externally-produced signature.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let unsigned = near.transaction("bob.testnet")
    ///     .transfer(NearToken::from_near(1))
    ///     .build()
    ///     .await?;
    ///
    /// // Get the hash that needs signing
    /// let hash = unsigned.get_hash();
    ///
    /// // Sign externally (Ledger, MPC, HSM, etc.)
    /// // let sig_bytes = device.sign(hash.as_bytes())?;
    /// // let signature = Signature::from_parts(KeyType::ED25519, &sig_bytes)?;
    ///
    /// // Complete and submit
    /// // let signed = unsigned.complete(signature);
    /// // near.send(&signed).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build(self) -> Result<Transaction, Error> {
        let prepared = self.prepare("Transaction must have at least one action")?;
        // Do not claim a rotating key merely to build an unsigned transaction.
        let (signer_id, public_key) = prepared.peek_signer()?;

        let span = trace::info_span!(
            "build_transaction",
            sender = %signer_id,
            receiver = %prepared.receiver_id,
            action_count = prepared.actions.len(),
            actions = %actions_summary(&prepared.actions),
            method = trace::field::Empty,
            gas = trace::field::Empty,
            deposit = trace::field::Empty,
        );

        async move {
            #[cfg(feature = "tracing")]
            record_function_call_fields(&prepared.actions);

            let (nonce, block_hash) = prepared
                .online_context(&signer_id, &public_key, None)
                .await?;
            let tx = prepared.into_transaction(signer_id, public_key, nonce, block_hash);

            trace::debug!(tx_hash = %tx.get_hash(), nonce, "Transaction built (unsigned)");

            Ok(tx)
        }
        .instrument(span)
        .await
    }

    /// Build an unsigned transaction offline without network access or a signer.
    ///
    /// Use this for fully air-gapped workflows where you provide all
    /// transaction metadata manually.
    ///
    /// # Arguments
    ///
    /// * `signer_id` - The account that will sign and pay for the transaction
    /// * `public_key` - The public key of the signer
    /// * `block_hash` - A recent block hash (transaction expires ~24h after this block)
    /// * `nonce` - The next nonce for the signing key (current nonce + 1)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::signer::PublicKey;
    /// # fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let block_hash = CryptoHash::ZERO;
    /// let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse().unwrap();
    ///
    /// let unsigned = near.transaction("bob.testnet")
    ///     .transfer(NearToken::from_near(1))
    ///     .build_offline("alice.testnet", public_key, block_hash, 12345)?;
    ///
    /// let hash = unsigned.get_hash();
    /// // Sign hash externally, then call unsigned.complete(signature)
    /// # Ok(())
    /// # }
    /// ```
    pub fn build_offline(
        self,
        signer_id: impl TryIntoAccountId,
        public_key: PublicKey,
        block_hash: CryptoHash,
        nonce: u64,
    ) -> Result<Transaction, Error> {
        let prepared = self.prepare("Transaction must have at least one action")?;
        let signer_id: AccountId = signer_id.try_into_account_id()?;

        Ok(prepared.into_transaction(signer_id, public_key, nonce, block_hash))
    }

    /// Take the validated receiver and actions without building a transaction.
    ///
    /// Use this to hand a builder's actions to something other than a signed
    /// transaction, such as a contract-side promise or a proposal payload. No
    /// signer, nonce, or network access is involved.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let (receiver_id, actions) = near.transaction("contract.testnet")
    ///     .call("method")
    ///         .args(serde_json::json!({"key": "value"}))
    ///         .deposit(NearToken::from_yoctonear(1))
    ///     .finish()
    ///     .transfer(NearToken::from_near(1))
    ///     .into_parts()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the receiver or first deferred builder error, or
    /// [`Error::InvalidTransaction`] if no actions were added.
    pub fn into_parts(self) -> Result<(AccountId, Vec<Action>), Error> {
        let prepared = self.prepare("Transaction must have at least one action")?;
        Ok((prepared.receiver_id, prepared.actions))
    }

    /// Sign the transaction without sending it.
    ///
    /// Returns a `SignedTransaction` that can be inspected or sent later.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let signed = near.transaction("bob.testnet")
    ///     .transfer(NearToken::from_near(1))
    ///     .sign()
    ///     .await?;
    ///
    /// // Inspect the transaction
    /// println!("Hash: {}", signed.transaction.get_hash());
    /// println!("Actions: {:?}", signed.transaction.actions);
    ///
    /// // Send it later
    /// let outcome = near.send(&signed).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sign(self) -> Result<SignedTransaction, Error> {
        let prepared = self.prepare("Transaction must have at least one action")?;
        let (signer_id, key) = prepared.claim_signer()?;
        let public_key = key.public_key().clone();

        let span = trace::info_span!(
            "sign_transaction",
            sender = %signer_id,
            receiver = %prepared.receiver_id,
            action_count = prepared.actions.len(),
            actions = %actions_summary(&prepared.actions),
            method = trace::field::Empty,
            gas = trace::field::Empty,
            deposit = trace::field::Empty,
        );

        async move {
            #[cfg(feature = "tracing")]
            record_function_call_fields(&prepared.actions);

            let (nonce, block_hash) = prepared
                .online_context(&signer_id, &public_key, None)
                .await?;
            let tx = prepared.into_transaction(signer_id, public_key, nonce, block_hash);
            let (signed_tx, _tx_hash) = PreparedTransaction::sign_transaction(tx, &key).await?;

            trace::debug!(tx_hash = %_tx_hash, nonce, "Transaction signed");

            Ok(signed_tx)
        }
        .instrument(span)
        .await
    }

    /// Sign the transaction offline without network access.
    ///
    /// This is useful for air-gapped signing workflows where you need to
    /// provide the block hash and nonce manually (obtained from a separate
    /// online machine).
    ///
    /// # Arguments
    ///
    /// * `block_hash` - A recent block hash (transaction expires ~24h after this block)
    /// * `nonce` - The next nonce for the signing key (current nonce + 1)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// # use near_kit::*;
    /// // On online machine: get block hash and nonce
    /// // let block = near.rpc().block(BlockReference::latest()).await?;
    /// // let access_key = near.rpc().view_access_key(...).await?;
    ///
    /// // On offline machine: sign with pre-fetched values
    /// let block_hash: CryptoHash = "11111111111111111111111111111111".parse().unwrap();
    /// let nonce = 12345u64;
    ///
    /// let signed = near.transaction("bob.testnet")
    ///     .transfer(NearToken::from_near(1))
    ///     .sign_offline(block_hash, nonce)
    ///     .await?;
    ///
    /// // Transport signed_tx.to_base64() back to online machine
    /// ```
    pub async fn sign_offline(
        self,
        block_hash: CryptoHash,
        nonce: u64,
    ) -> Result<SignedTransaction, Error> {
        let prepared = self.prepare("Transaction must have at least one action")?;
        let (signer_id, key) = prepared.claim_signer()?;
        let public_key = key.public_key().clone();

        let tx = prepared.into_transaction(signer_id, public_key, nonce, block_hash);
        PreparedTransaction::sign_transaction(tx, &key)
            .await
            .map(|(signed_tx, _)| signed_tx)
    }

    /// Send the transaction.
    ///
    /// Returns a [`TransactionSend`] that defaults to [`crate::transaction::ExecutedOptimistic`] wait level.
    /// Chain `.wait_until::<W>()` to change the wait level before awaiting.
    pub fn send(self) -> TransactionSend {
        TransactionSend {
            builder: self,
            _marker: PhantomData,
        }
    }
}

// ============================================================================
// CallBuilder
// ============================================================================

/// Builder for configuring a function call within a transaction.
///
/// Created via [`TransactionBuilder::call`]. Allows setting args, gas, and deposit
/// before extracting the action, returning to the transaction with
/// [`finish`](Self::finish), or sending it as a one-shot call.
pub struct CallBuilder {
    builder: TransactionBuilder,
    call: FunctionCall,
}

impl fmt::Debug for CallBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallBuilder")
            .field("call", &self.call)
            .field("builder", &self.builder)
            .finish()
    }
}

impl CallBuilder {
    fn new(builder: TransactionBuilder, method: String) -> Self {
        Self {
            builder,
            call: FunctionCall::new(method),
        }
    }

    /// Set JSON arguments.
    pub fn args<A: serde::Serialize>(mut self, args: A) -> Self {
        self.call = self.call.args(args);
        self
    }

    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.call = self.call.args_raw(args);
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh<A: borsh::BorshSerialize>(mut self, args: A) -> Self {
        self.call = self.call.args_borsh(args);
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
    ///         .gas(Gas::from_tgas(50))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Parsing failures are retained and returned when the transaction is sent,
    /// built, signed, delegated, or converted into an action.
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        self.call = self.call.gas(gas);
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
    ///         .deposit(NearToken::from_near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Parsing failures are retained and returned when the transaction is sent,
    /// built, signed, delegated, or converted into an action.
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        self.call = self.call.deposit(amount);
        self
    }

    /// Convert this call into a standalone [`Action`], discarding the
    /// underlying transaction builder.
    ///
    /// This is useful for extracting a typed contract call so it can be
    /// composed into a different transaction.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// // Extract actions from the fluent builder
    /// let action = near.transaction("contract.testnet")
    ///     .call("method")
    ///     .args(serde_json::json!({"key": "value"}))
    ///     .gas(Gas::from_tgas(50))
    ///     .into_action()?;
    ///
    /// // Compose into a different transaction
    /// near.transaction("contract.testnet")
    ///     .add_action(action)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the first deferred builder/call input error, or
    /// [`Error::InvalidTransaction`] if the underlying builder already has
    /// accumulated actions that would otherwise be discarded. Use
    /// [`finish`](Self::finish) to keep them in the transaction.
    pub fn into_action(self) -> Result<Action, Error> {
        let Self { builder, call } = self;
        builder.receiver_id?;
        if let Some(error) = builder.construction_error {
            return Err(error);
        }
        if !builder.actions.is_empty() {
            return Err(Error::InvalidTransaction(format!(
                "into_action() would discard {} previously accumulated action(s); use .finish() to keep them in the transaction",
                builder.actions.len(),
            )));
        }
        call.into_action()
    }

    /// Finish this call and return to the transaction builder.
    ///
    /// This is useful when you need to conditionally add actions to a
    /// transaction, since it gives back the [`TransactionBuilder`] so you can
    /// branch on runtime state before starting the next action. Transaction
    /// configuration and additional actions are available on that returned
    /// builder rather than being duplicated here.
    pub fn finish(self) -> TransactionBuilder {
        self.builder.add_action(self.call)
    }

    /// Finish this call and send its transaction.
    pub fn send(self) -> TransactionSend {
        self.finish().send()
    }
}

impl IntoFuture for CallBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        self.send().into_future()
    }
}

// ============================================================================
// SignedTransactionSend
// ============================================================================

/// Awaitable builder for sending a pre-signed transaction.
///
/// The type parameter `W` determines the wait level and response type. Awaiting
/// this builder directly uses [`ExecutedOptimistic`](crate::transaction::ExecutedOptimistic),
/// while [`wait_until`](Self::wait_until) selects a different wait level at compile time.
///
/// The builder borrows the signed transaction, so the future produced by
/// [`IntoFuture`] is tied to the `'tx` lifetime rather than `'static`. To
/// satisfy a `'static` bound (for example `tokio::spawn`), wrap the send in an
/// `async move` block that owns the transaction.
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # use near_kit::protocol::SignedTransaction;
/// # use near_kit::rpc::SendTxResponse;
/// # use near_kit::transaction::Included;
/// # async fn example(near: &Near, signed: &SignedTransaction) -> Result<(), Error> {
/// // Early wait levels return SendTxResponse, useful for fire-and-track
/// // workflows. Awaiting directly without `.wait_until` uses the
/// // ExecutedOptimistic default and returns FinalExecutionOutcome instead.
/// let submitted: SendTxResponse = near.send(signed)
///     .wait_until::<Included>()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[must_use = "signed transactions are not sent unless awaited"]
pub struct SignedTransactionSend<'tx, W: WaitLevel = crate::types::ExecutedOptimistic> {
    rpc: Arc<RpcClient>,
    signed_tx: &'tx SignedTransaction,
    _marker: PhantomData<W>,
}

impl<'tx> SignedTransactionSend<'tx> {
    pub(crate) fn new(rpc: Arc<RpcClient>, signed_tx: &'tx SignedTransaction) -> Self {
        Self {
            rpc,
            signed_tx,
            _marker: PhantomData,
        }
    }
}

impl<'tx, W: WaitLevel> SignedTransactionSend<'tx, W> {
    /// Select the execution wait level and corresponding response type.
    pub fn wait_until<W2: WaitLevel>(self) -> SignedTransactionSend<'tx, W2> {
        SignedTransactionSend {
            rpc: self.rpc,
            signed_tx: self.signed_tx,
            _marker: PhantomData,
        }
    }
}

impl<'tx, W: WaitLevel> IntoFuture for SignedTransactionSend<'tx, W> {
    type Output = Result<W::Response, Error>;
    type IntoFuture = crate::platform::BoxFuture<'tx, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let sender_id = &self.signed_tx.transaction.signer_id;
            let response = self.rpc.send_tx(self.signed_tx, W::STATUS).await?;
            W::convert(response, sender_id)
        })
    }
}

// ============================================================================
// TransactionSend
// ============================================================================

/// Future for sending a transaction.
///
/// The type parameter `W` determines the wait level and the return type:
/// - Executed levels ([`crate::transaction::ExecutedOptimistic`], [`crate::transaction::Executed`],
///   [`crate::transaction::Final`]) → [`FinalExecutionOutcome`]
/// - Non-executed levels ([`crate::transaction::Submitted`], [`crate::transaction::Included`],
///   [`crate::transaction::IncludedFinal`]) → [`crate::rpc::SendTxResponse`]
#[must_use = "transactions are not sent unless awaited"]
pub struct TransactionSend<W: WaitLevel = crate::types::ExecutedOptimistic> {
    builder: TransactionBuilder,
    _marker: PhantomData<W>,
}

impl<W: WaitLevel> TransactionSend<W> {
    /// Change the execution wait level.
    ///
    /// The return type changes based on the wait level:
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::transaction::{Final, Included};
    /// # async fn example(near: &Near) -> Result<(), Error> {
    /// // Executed levels return FinalExecutionOutcome
    /// let outcome = near.transfer("bob.testnet", NearToken::from_near(1))
    ///     .send()
    ///     .wait_until::<Final>()
    ///     .await?;
    ///
    /// // Non-executed levels return SendTxResponse
    /// let response = near.transfer("bob.testnet", NearToken::from_near(1))
    ///     .send()
    ///     .wait_until::<Included>()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn wait_until<W2: WaitLevel>(self) -> TransactionSend<W2> {
        TransactionSend {
            builder: self.builder,
            _marker: PhantomData,
        }
    }

    /// Override the number of nonce retries for this transaction on `InvalidNonce`
    /// errors. `0` means no retries (send once), `1` means one retry, etc.
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.builder.max_nonce_retries = retries;
        self
    }
}

impl<W: WaitLevel> IntoFuture for TransactionSend<W> {
    type Output = Result<W::Response, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let prepared = self
                .builder
                .prepare("Transaction must have at least one action")?;
            // Claim one key for the entire logical send. Retries must keep
            // nonce state and signatures paired with that same access key.
            let (signer_id, key) = prepared.claim_signer()?;
            let public_key = key.public_key().clone();

            let span = trace::info_span!(
                "send_transaction",
                sender = %signer_id,
                receiver = %prepared.receiver_id,
                action_count = prepared.actions.len(),
                actions = %actions_summary(&prepared.actions),
                method = trace::field::Empty,
                gas = trace::field::Empty,
                deposit = trace::field::Empty,
            );

            async move {
                #[cfg(feature = "tracing")]
                record_function_call_fields(&prepared.actions);

                // Retry loop for transient InvalidTxErrors (nonce conflicts, expired block hash)
                let max_nonce_retries = prepared.max_nonce_retries;
                let wait_until = W::STATUS;
                let mut last_error: Option<Error> = None;
                let mut last_ak_nonce: Option<u64> = None;

                for attempt in 0..=max_nonce_retries {
                    let (nonce, block_hash) = prepared
                        .online_context(&signer_id, &public_key, last_ak_nonce.take())
                        .await?;

                    let tx = prepared.transaction(
                        signer_id.clone(),
                        public_key.clone(),
                        nonce,
                        block_hash,
                    );
                    let (signed_tx, _) = PreparedTransaction::sign_transaction(tx, &key).await?;

                    // Send
                    match prepared.rpc.send_tx(&signed_tx, wait_until).await {
                        Ok(response) => {
                            // W::convert handles the response appropriately:
                            // - Executed levels: extract outcome, check for InvalidTxError
                            // - Non-executed levels: build SendTxResponse (hash + sender,
                            //   plus SendTxResponse::outcome). On this send_tx path the
                            //   node returns no outcome at early levels, so it stays None;
                            //   the tx_status path is where outcome can be Some.
                            return W::convert(response, &signer_id);
                        }
                        Err(RpcError::InvalidTx(
                            crate::types::InvalidTxError::InvalidNonce { tx_nonce, ak_nonce },
                        )) if attempt < max_nonce_retries => {
                            trace::debug!(
                                tx_nonce = tx_nonce,
                                ak_nonce = ak_nonce,
                                attempt = attempt + 1,
                                "Invalid nonce, retrying"
                            );
                            // Store ak_nonce for next iteration to avoid refetching
                            last_ak_nonce = Some(ak_nonce);
                            last_error = Some(Error::InvalidTx(Box::new(
                                crate::types::InvalidTxError::InvalidNonce { tx_nonce, ak_nonce },
                            )));
                            continue;
                        }
                        Err(RpcError::InvalidTx(crate::types::InvalidTxError::Expired))
                            if attempt < max_nonce_retries =>
                        {
                            trace::debug!(
                                attempt = attempt + 1,
                                "Transaction expired (stale block hash), retrying with fresh block hash"
                            );
                            // Expired tx was rejected before nonce consumption.
                            // No cache invalidation needed: the next iteration calls
                            // view_access_key which provides a fresh nonce, and
                            // next() uses max(cached, chain) so stale cache is harmless.
                            last_error = Some(Error::InvalidTx(Box::new(
                                crate::types::InvalidTxError::Expired,
                            )));
                            continue;
                        }
                        Err(e) => {
                            // DEBUG, not ERROR: the error is returned to the
                            // caller (see `RpcClient::call`).
                            trace::debug!(error = %e, "Transaction send failed");
                            return Err(e.into());
                        }
                    }
                }

                Err(last_error.unwrap_or_else(|| {
                    Error::InvalidTransaction("Unknown error during transaction send".to_string())
                }))
            }
            .instrument(span)
            .await
        })
    }
}

impl IntoFuture for TransactionBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        self.send().into_future()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{self, Write};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::client::{
        BoxFuture, InMemorySigner, RotatingSigner, RpcTransport, TransportResponse,
    };
    use crate::types::{Gas, SecretKey, Submitted};

    struct FailingJson;

    impl serde::Serialize for FailingJson {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("deliberate JSON failure"))
        }
    }

    struct FailingBorsh;

    impl borsh::BorshSerialize for FailingBorsh {
        fn serialize<W: Write>(&self, _writer: &mut W) -> io::Result<()> {
            Err(io::Error::other("deliberate Borsh failure"))
        }
    }

    /// Create a TransactionBuilder for unit tests (no real network needed).
    fn test_builder() -> TransactionBuilder {
        let rpc = Arc::new(RpcClient::new("https://rpc.testnet.near.org"));
        let receiver: AccountId = "contract.testnet".parse().unwrap();
        TransactionBuilder::new_fallible(rpc, None, Ok(receiver), 0)
    }

    #[tokio::test]
    async fn receiver_overrides_preserve_invalid_receiver_errors() {
        let near = crate::Near::testnet().build();
        let key = SecretKey::generate_ed25519();
        let delegate = crate::types::DelegateAction {
            sender_id: "sender.testnet".parse().unwrap(),
            receiver_id: "receiver.testnet".parse().unwrap(),
            actions: vec![
                NonDelegateAction::from_action(Action::transfer(NearToken::ZERO)).unwrap(),
            ],
            nonce: 1,
            max_block_height: 100,
            public_key: key.public_key(),
        };
        let signature = key.sign(delegate.get_hash().as_bytes());
        let signed_delegate = delegate.sign(signature);
        let state_init = <StateInit as crate::types::StateInitExt>::by_hash(
            CryptoHash::ZERO,
            Default::default(),
        );

        for terminal in 0..4 {
            for builder in [
                near.transaction("INVALID")
                    .state_init(state_init.clone(), NearToken::ZERO),
                near.transaction("INVALID")
                    .signed_delegate_action(signed_delegate.clone()),
            ] {
                let error = match terminal {
                    0 => builder.send().await.unwrap_err(),
                    1 => builder.sign().await.unwrap_err(),
                    2 => builder.delegate(Default::default()).await.unwrap_err(),
                    _ => builder
                        .build_offline("signer.testnet", key.public_key(), CryptoHash::ZERO, 1)
                        .unwrap_err(),
                };
                assert!(matches!(error, Error::ParseAccountId(_)), "{error:?}");
            }
        }

        // Successful receiver overrides still target the derived account or sender.
        for (builder, expected) in [
            (
                near.transaction("valid.testnet")
                    .state_init(state_init.clone(), NearToken::ZERO),
                state_init.derive_account_id(),
            ),
            (
                near.transaction("valid.testnet")
                    .signed_delegate_action(signed_delegate.clone()),
                signed_delegate.sender_id().clone(),
            ),
        ] {
            let transaction = builder
                .build_offline("signer.testnet", key.public_key(), CryptoHash::ZERO, 1)
                .unwrap();
            assert_eq!(transaction.receiver_id, expected);
        }
    }

    #[test]
    fn add_action_appends_to_transaction() {
        let action = Action::function_call(
            "do_something",
            serde_json::to_vec(&serde_json::json!({ "key": "value" })).unwrap(),
            Gas::from_tgas(30),
            NearToken::ZERO,
        );

        let builder = test_builder().add_action(action);
        assert_eq!(builder.actions.len(), 1);
    }

    #[test]
    fn add_action_chains_with_other_actions() {
        let call_action =
            Action::function_call("init", Vec::new(), Gas::from_tgas(10), NearToken::ZERO);

        let builder = test_builder()
            .create_account()
            .transfer(NearToken::from_near(5))
            .add_action(call_action);

        assert_eq!(builder.actions.len(), 3);
    }

    #[test]
    fn finish_returns_to_transaction_composition() {
        let extra_action = Action::transfer(NearToken::from_near(1));

        let builder = test_builder()
            .call("setup")
            .args(serde_json::json!({ "admin": "alice.testnet" }))
            .gas(Gas::from_tgas(50))
            .finish()
            .add_action(extra_action);

        // Should have two actions: the function call from CallBuilder + the transfer
        assert_eq!(builder.actions.len(), 2);
    }

    #[tokio::test]
    async fn call_builder_defers_serialization_error_until_send() {
        let error = test_builder()
            .call("method")
            .args(FailingJson)
            .send()
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Json(_)));
    }

    #[tokio::test]
    async fn transaction_amount_and_call_input_errors_are_deferred() {
        let transfer_error = test_builder()
            .transfer("definitely not NEAR")
            .send()
            .await
            .unwrap_err();
        assert!(matches!(transfer_error, Error::ParseAmount(_)));

        let stake_error = test_builder()
            .stake(
                "definitely not NEAR",
                SecretKey::generate_ed25519().public_key(),
            )
            .send()
            .await
            .unwrap_err();
        assert!(matches!(stake_error, Error::ParseAmount(_)));

        let call_error = test_builder()
            .call("method")
            .deposit("definitely not NEAR")
            .gas("definitely not gas")
            .send()
            .await
            .unwrap_err();
        assert!(matches!(call_error, Error::ParseAmount(_)));

        let state_init = <StateInit as crate::types::StateInitExt>::by_hash(
            CryptoHash::ZERO,
            Default::default(),
        );
        let state_init_error = test_builder()
            .state_init(state_init, "definitely not NEAR")
            .send()
            .await
            .unwrap_err();
        assert!(matches!(state_init_error, Error::ParseAmount(_)));
    }

    #[tokio::test]
    async fn transaction_input_errors_reach_sign_and_delegate() {
        let sign_error = test_builder()
            .transfer("definitely not NEAR")
            .sign()
            .await
            .unwrap_err();
        assert!(matches!(sign_error, Error::ParseAmount(_)));

        let delegate_error = test_builder()
            .transfer("definitely not NEAR")
            .delegate(Default::default())
            .await
            .unwrap_err();
        assert!(matches!(delegate_error, Error::ParseAmount(_)));
    }

    #[test]
    fn composed_function_call_defers_serialization_error_until_build() {
        let public_key = SecretKey::generate_ed25519().public_key();
        let error = test_builder()
            .add_action(FunctionCall::new("method").args_borsh(FailingBorsh))
            .build_offline("alice.testnet", public_key, CryptoHash::ZERO, 1)
            .unwrap_err();

        assert!(matches!(error, Error::Borsh(_)));
    }

    #[test]
    fn function_call_compose_into_transaction() {
        let init = FunctionCall::new("init")
            .args(serde_json::json!({"owner": "alice.testnet"}))
            .gas(Gas::from_tgas(50));

        let notify = FunctionCall::new("notify").args(serde_json::json!({"msg": "done"}));

        let builder = test_builder()
            .deploy(vec![0u8])
            .add_action(init)
            .add_action(notify);

        assert_eq!(builder.actions.len(), 3);
    }

    #[test]
    fn function_call_dynamic_loop_composition() {
        let methods = vec!["step1", "step2", "step3"];

        let mut tx = test_builder();
        for method in methods {
            tx = tx.add_action(FunctionCall::new(method));
        }

        assert_eq!(tx.actions.len(), 3);
    }

    #[test]
    fn call_builder_into_action() {
        let action = test_builder()
            .call("setup")
            .args(serde_json::json!({"admin": "alice.testnet"}))
            .gas(Gas::from_tgas(50))
            .deposit(NearToken::from_near(1))
            .into_action()
            .unwrap();

        match &action {
            Action::FunctionCall(fc) => {
                assert_eq!(fc.method_name, "setup");
                assert_eq!(fc.gas, Gas::from_tgas(50));
                assert_eq!(fc.deposit, NearToken::from_near(1));
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn call_builder_into_action_compose() {
        let action1 = test_builder()
            .call("method_a")
            .gas(Gas::from_tgas(50))
            .into_action()
            .unwrap();

        let action2 = test_builder()
            .call("method_b")
            .deposit(NearToken::from_near(1))
            .into_action()
            .unwrap();

        let builder = test_builder().add_action(action1).add_action(action2);

        assert_eq!(builder.actions.len(), 2);
    }

    #[test]
    fn call_builder_into_action_rejects_accumulated_actions() {
        let error = test_builder()
            .transfer(NearToken::from_near(1))
            .call("method")
            .into_action()
            .unwrap_err();

        assert!(matches!(error, Error::InvalidTransaction(_)));
    }

    #[test]
    fn call_builder_into_action_preserves_builder_construction_error() {
        let error = test_builder()
            .deploy_from("INVALID")
            .call("method")
            .into_action()
            .unwrap_err();

        assert!(matches!(error, Error::ParseAccountId(_)));
    }

    #[test]
    fn into_parts_returns_receiver_and_actions() {
        let (receiver_id, actions) = test_builder()
            .call("method")
            .deposit(NearToken::from_yoctonear(1))
            .finish()
            .transfer(NearToken::from_near(1))
            .into_parts()
            .unwrap();

        assert_eq!(receiver_id.as_str(), "contract.testnet");
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], Action::FunctionCall(fc) if fc.method_name == "method"));
        assert!(matches!(&actions[1], Action::Transfer(_)));
    }

    #[test]
    fn into_parts_rejects_empty_builder() {
        let error = test_builder().into_parts().unwrap_err();

        assert!(matches!(error, Error::InvalidTransaction(_)));
    }

    #[test]
    fn into_parts_preserves_deferred_errors() {
        let error = test_builder()
            .call("method")
            .args(FailingJson)
            .finish()
            .into_parts()
            .unwrap_err();
        assert!(matches!(error, Error::Json(_)));

        let rpc = Arc::new(RpcClient::new("https://rpc.testnet.near.org"));
        let error = TransactionBuilder::new_fallible(
            rpc,
            None,
            "INVALID".parse::<AccountId>().map_err(Error::from),
            0,
        )
        .transfer(NearToken::from_near(1))
        .into_parts()
        .unwrap_err();
        assert!(matches!(error, Error::ParseAccountId(_)));
    }

    // ========================================================================
    // Nonce refresh + re-sign on InvalidNonce (mock transport)
    // ========================================================================

    /// Transport that answers `EXPERIMENTAL_view_access_key` with a fixed key
    /// (nonce 5), scripts `send_tx` responses in order, and records every
    /// signed payload it received.
    struct NonceRetryTransport {
        send_tx_responses: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<String>>,
        access_key_queries: AtomicUsize,
    }

    impl NonceRetryTransport {
        fn new(send_tx_responses: Vec<Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                send_tx_responses: Mutex::new(send_tx_responses.into()),
                sent: Mutex::new(Vec::new()),
                access_key_queries: AtomicUsize::new(0),
            })
        }
    }

    impl RpcTransport for NonceRetryTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let body = match request["method"].as_str().unwrap() {
                "EXPERIMENTAL_view_access_key" => {
                    self.access_key_queries.fetch_add(1, Ordering::SeqCst);
                    serde_json::to_vec(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 0,
                        "result": {
                            "nonce": 5,
                            "permission": "FullAccess",
                            "block_height": 100,
                            "block_hash": "11111111111111111111111111111111",
                        },
                    }))
                    .unwrap()
                }
                "send_tx" => {
                    let signed = request["params"]["signed_tx_base64"]
                        .as_str()
                        .unwrap()
                        .to_string();
                    self.sent.lock().unwrap().push(signed);
                    self.send_tx_responses
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("unexpected extra send_tx")
                }
                other => panic!("unexpected RPC method {other}"),
            };
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    fn invalid_nonce_body(tx_nonce: u64, ak_nonce: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": {
                "name": "HANDLER_ERROR",
                "cause": { "name": "INVALID_TRANSACTION", "info": {} },
                "code": -32000,
                "message": "Server error",
                "data": {
                    "TxExecutionError": {
                        "InvalidTxError": {
                            "InvalidNonce": { "tx_nonce": tx_nonce, "ak_nonce": ak_nonce }
                        }
                    }
                },
            },
        }))
        .unwrap()
    }

    fn accepted_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": { "final_execution_status": "NONE" },
        }))
        .unwrap()
    }

    fn expired_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": {
                "name": "HANDLER_ERROR",
                "cause": { "name": "INVALID_TRANSACTION", "info": {} },
                "code": -32000,
                "message": "Server error",
                "data": {
                    "TxExecutionError": {
                        "InvalidTxError": "Expired"
                    }
                },
            },
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn build_peeks_without_claiming_a_rotating_key() {
        let transport = NonceRetryTransport::new(vec![]);
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let first_public_key = keys[0].public_key();
        let signer = RotatingSigner::new("alice.testnet", keys).unwrap();
        let near = crate::Near::custom("http://build-peek.invalid", "test")
            .transport(transport)
            .signer(signer)
            .build();

        let unsigned = near
            .transfer("bob.testnet", NearToken::from_near(1))
            .build()
            .await
            .unwrap();
        let signed = near
            .transfer("bob.testnet", NearToken::from_near(1))
            .sign_offline(CryptoHash::ZERO, 1)
            .await
            .unwrap();

        assert_eq!(unsigned.public_key, first_public_key);
        assert_eq!(signed.transaction.public_key, first_public_key);
    }

    #[tokio::test]
    async fn send_refreshes_nonce_and_resigns_on_invalid_nonce() {
        // First send_tx is rejected with InvalidNonce (the key's real nonce is
        // 20); the second is accepted.
        let transport = NonceRetryTransport::new(vec![invalid_nonce_body(6, 20), accepted_body()]);
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let signer = RotatingSigner::new("alice.testnet", keys).unwrap();
        // Default RetryConfig on purpose: the raw RPC layer must not add
        // identical resends of the rejected payload before this loop runs.
        let near = crate::Near::custom("http://mock.invalid", "test")
            .transport(transport.clone())
            .signer(signer)
            .build();

        near.transfer("bob.testnet", NearToken::from_near(1))
            .wait_until::<Submitted>()
            .await
            .expect("second attempt is accepted");

        let sent = transport.sent.lock().unwrap();
        assert_eq!(sent.len(), 2, "exactly one InvalidNonce retry");
        let first = SignedTransaction::from_base64(&sent[0]).unwrap();
        let second = SignedTransaction::from_base64(&sent[1]).unwrap();
        // First attempt: access key nonce (5) + 1. Retry: ak_nonce from the
        // error (20) + 1, freshly signed.
        assert_eq!(first.transaction.nonce, 6);
        assert_eq!(second.transaction.nonce, 21);
        assert_eq!(
            first.transaction.public_key, second.transaction.public_key,
            "one logical send must retain its claimed rotating key across retries"
        );
        assert_ne!(first.signature, second.signature, "retry must be re-signed");
        assert_eq!(
            transport.access_key_queries.load(Ordering::SeqCst),
            2,
            "each attempt re-fetches the access key for a fresh block hash"
        );
    }

    #[tokio::test]
    async fn expired_retries_honor_configured_maximum() {
        let transport =
            NonceRetryTransport::new(vec![expired_body(), expired_body(), accepted_body()]);
        let signer =
            InMemorySigner::from_secret_key("alice.testnet", SecretKey::generate_ed25519())
                .unwrap();
        let near = crate::Near::custom("http://expired-retry.invalid", "test")
            .transport(transport.clone())
            .signer(signer)
            .build();

        near.transfer("bob.testnet", NearToken::from_near(1))
            .max_nonce_retries(2)
            .wait_until::<Submitted>()
            .await
            .expect("two configured retries should allow the third send");

        assert_eq!(transport.sent.lock().unwrap().len(), 3);
        assert_eq!(transport.access_key_queries.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn expired_with_zero_retries_sends_once() {
        let transport = NonceRetryTransport::new(vec![expired_body()]);
        let signer =
            InMemorySigner::from_secret_key("alice.testnet", SecretKey::generate_ed25519())
                .unwrap();
        let near = crate::Near::custom("http://no-expired-retry.invalid", "test")
            .transport(transport.clone())
            .signer(signer)
            .build();

        let error = near
            .transfer("bob.testnet", NearToken::from_near(1))
            .max_nonce_retries(0)
            .wait_until::<Submitted>()
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::InvalidTx(error) if matches!(*error, crate::types::InvalidTxError::Expired)
        ));
        assert_eq!(transport.sent.lock().unwrap().len(), 1);
    }
}
