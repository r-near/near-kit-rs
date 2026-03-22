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

use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::error::{Error, RpcError};
use crate::types::{
    AccountId, Action, BlockReference, CryptoHash, DelegateAction, DeterministicAccountStateInit,
    FinalExecutionOutcome, Finality, Gas, IntoGas, IntoNearToken, NearToken, NonDelegateAction,
    PublicKey, SignedDelegateAction, SignedTransaction, Transaction, TryIntoAccountId,
    TxExecutionStatus,
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
/// Created via [`crate::Near::transaction`]. Supports chaining multiple actions
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
    receiver_id: AccountId,
    actions: Vec<Action>,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
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
            .field("wait_until", &self.wait_until)
            .field("max_nonce_retries", &self.max_nonce_retries)
            .finish()
    }
}

impl TransactionBuilder {
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        receiver_id: AccountId,
        max_nonce_retries: u32,
    ) -> Self {
        Self {
            rpc,
            signer,
            receiver_id,
            actions: Vec::new(),
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
            max_nonce_retries,
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
    ///     .transfer(NearToken::from_near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the amount string cannot be parsed.
    pub fn transfer(mut self, amount: impl IntoNearToken) -> Self {
        let amount = amount
            .into_near_token()
            .expect("invalid transfer amount - use NearToken::from_str() for user input");
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
    ///         .gas(Gas::from_tgas(10))
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
        receiver_id: impl TryIntoAccountId,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        let receiver_id = receiver_id
            .try_into_account_id()
            .expect("invalid account ID");
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
    pub fn delete_account(mut self, beneficiary_id: impl TryIntoAccountId) -> Self {
        let beneficiary_id = beneficiary_id
            .try_into_account_id()
            .expect("invalid account ID");
        self.actions.push(Action::delete_account(beneficiary_id));
        self
    }

    /// Add a stake action.
    ///
    /// # Panics
    ///
    /// Panics if the amount string cannot be parsed.
    pub fn stake(mut self, amount: impl IntoNearToken, public_key: PublicKey) -> Self {
        let amount = amount
            .into_near_token()
            .expect("invalid stake amount - use NearToken::from_str() for user input");
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
    ///     .transaction(signed_delegate.sender_id())
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
    ///         .gas(Gas::from_tgas(30))
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

        // Get a signing key atomically
        let key = signer.key();
        let public_key = key.public_key().clone();

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
    /// # async fn example(near: Near) -> Result<(), Box<dyn std::error::Error>> {
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
    pub fn deploy_from_publisher(mut self, publisher_id: impl TryIntoAccountId) -> Self {
        let publisher_id = publisher_id
            .try_into_account_id()
            .expect("invalid account ID");
        self.actions.push(Action::deploy_from_account(publisher_id));
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
    /// # async fn example(near: Near, code_hash: CryptoHash) -> Result<(), near_kit::Error> {
    /// let si = DeterministicAccountStateInit::by_hash(code_hash, Default::default());
    /// let outcome = near.transaction("alice.testnet")
    ///     .state_init(si, NearToken::from_near(1))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the deposit amount string cannot be parsed.
    pub fn state_init(
        mut self,
        state_init: DeterministicAccountStateInit,
        deposit: impl IntoNearToken,
    ) -> Self {
        let deposit = deposit
            .into_near_token()
            .expect("invalid deposit amount - use NearToken::from_str() for user input");

        self.receiver_id = state_init.derive_account_id();
        self.actions.push(Action::state_init(state_init, deposit));
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
    pub fn add_action(mut self, action: impl Into<Action>) -> Self {
        self.actions.push(action.into());
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

    /// Override the number of nonce retries for this transaction on `InvalidNonce`
    /// errors. `0` means no retries (send once), `1` means one retry, etc.
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.max_nonce_retries = retries;
        self
    }

    // ========================================================================
    // Execution
    // ========================================================================

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
        if self.actions.is_empty() {
            return Err(Error::InvalidTransaction(
                "Transaction must have at least one action".to_string(),
            ));
        }

        let signer = self
            .signer_override
            .or(self.signer)
            .ok_or(Error::NoSigner)?;

        let signer_id = signer.account_id().clone();
        let action_count = self.actions.len();

        tracing::info!(
            sender = %signer_id,
            receiver = %self.receiver_id,
            action_count = action_count,
            "Signing transaction"
        );

        // Get a signing key atomically. For RotatingSigner, this claims the next
        // key in rotation. The key contains both the public key and signing capability.
        let key = signer.key();
        let public_key = key.public_key().clone();
        let public_key_str = public_key.to_string();

        // Get nonce for the key
        let rpc = self.rpc.clone();
        let network = rpc.url().to_string();
        let signer_id_clone = signer_id.clone();
        let public_key_clone = public_key.clone();

        let nonce = nonce_manager()
            .get_next_nonce(&network, signer_id.as_ref(), &public_key_str, || async {
                let access_key = rpc
                    .view_access_key(
                        &signer_id_clone,
                        &public_key_clone,
                        BlockReference::Finality(Finality::Optimistic),
                    )
                    .await?;
                Ok(access_key.nonce)
            })
            .await?;

        // Get recent block hash
        let block = self
            .rpc
            .block(BlockReference::Finality(Finality::Final))
            .await?;

        // Build transaction
        let tx = Transaction::new(
            signer_id,
            public_key,
            nonce,
            self.receiver_id,
            block.header.hash,
            self.actions,
        );

        // Sign with the key
        let tx_hash = tx.get_hash();
        let signature = key.sign(tx_hash.as_bytes()).await?;

        tracing::info!(
            tx_hash = %tx_hash,
            nonce = nonce,
            "Transaction signed"
        );

        Ok(SignedTransaction {
            transaction: tx,
            signature,
        })
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
        if self.actions.is_empty() {
            return Err(Error::InvalidTransaction(
                "Transaction must have at least one action".to_string(),
            ));
        }

        let signer = self
            .signer_override
            .or(self.signer)
            .ok_or(Error::NoSigner)?;

        let signer_id = signer.account_id().clone();

        // Get a signing key atomically
        let key = signer.key();
        let public_key = key.public_key().clone();

        // Build transaction with provided block_hash and nonce
        let tx = Transaction::new(
            signer_id,
            public_key,
            nonce,
            self.receiver_id,
            block_hash,
            self.actions,
        );

        // Sign
        let signature = key.sign(tx.get_hash().as_bytes()).await?;

        Ok(SignedTransaction {
            transaction: tx,
            signature,
        })
    }

    /// Send the transaction.
    ///
    /// This is equivalent to awaiting the builder directly.
    pub fn send(self) -> TransactionSend {
        TransactionSend { builder: self }
    }
}

// ============================================================================
// FunctionCall
// ============================================================================

/// A standalone function call configuration, decoupled from any transaction.
///
/// Use this when you need to pre-build calls and compose them into a transaction
/// later. This is especially useful for dynamic transaction composition (e.g. in
/// a loop) or for batching typed contract calls into a single transaction.
///
/// Note: `FunctionCall` does not capture a receiver/contract account. The call
/// will execute against whichever `receiver_id` is set on the transaction it's
/// added to.
///
/// # Examples
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example(near: Near) -> Result<(), near_kit::Error> {
/// // Pre-build calls independently
/// let init = FunctionCall::new("init")
///     .args(serde_json::json!({"owner": "alice.testnet"}))
///     .gas(Gas::from_tgas(50));
///
/// let notify = FunctionCall::new("notify")
///     .args(serde_json::json!({"msg": "done"}));
///
/// // Compose into a single atomic transaction
/// near.transaction("contract.testnet")
///     .add_action(init)
///     .add_action(notify)
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// ```rust,no_run
/// # use near_kit::*;
/// # async fn example(near: Near) -> Result<(), near_kit::Error> {
/// // Dynamic composition in a loop
/// let calls = vec![
///     FunctionCall::new("method_a").args(serde_json::json!({"x": 1})),
///     FunctionCall::new("method_b").args(serde_json::json!({"y": 2})),
/// ];
///
/// let mut tx = near.transaction("contract.testnet");
/// for call in calls {
///     tx = tx.add_action(call);
/// }
/// tx.send().await?;
/// # Ok(())
/// # }
/// ```
pub struct FunctionCall {
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
}

impl fmt::Debug for FunctionCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionCall")
            .field("method", &self.method)
            .field("args_len", &self.args.len())
            .field("gas", &self.gas)
            .field("deposit", &self.deposit)
            .finish()
    }
}

impl FunctionCall {
    /// Create a new function call for the given method name.
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args: Vec::new(),
            gas: Gas::from_tgas(30),
            deposit: NearToken::ZERO,
        }
    }

    /// Set JSON arguments.
    pub fn args(mut self, args: impl serde::Serialize) -> Self {
        self.args = serde_json::to_vec(&args).unwrap_or_default();
        self
    }

    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: Vec<u8>) -> Self {
        self.args = args;
        self
    }

    /// Set Borsh-encoded arguments.
    pub fn args_borsh(mut self, args: impl borsh::BorshSerialize) -> Self {
        self.args = borsh::to_vec(&args).unwrap_or_default();
        self
    }

    /// Set gas limit.
    ///
    /// Defaults to 30 TGas if not set.
    ///
    /// # Panics
    ///
    /// Panics if the gas string cannot be parsed. Use [`Gas`]'s `FromStr` impl
    /// for fallible parsing of user input.
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        self.gas = gas
            .into_gas()
            .expect("invalid gas format - use Gas::from_str() for user input");
        self
    }

    /// Set attached deposit.
    ///
    /// Defaults to zero if not set.
    ///
    /// # Panics
    ///
    /// Panics if the amount string cannot be parsed. Use [`NearToken`]'s `FromStr`
    /// impl for fallible parsing of user input.
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        self.deposit = amount
            .into_near_token()
            .expect("invalid deposit amount - use NearToken::from_str() for user input");
        self
    }
}

impl From<FunctionCall> for Action {
    fn from(call: FunctionCall) -> Self {
        Action::function_call(call.method, call.args, call.gas, call.deposit)
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
    /// # Panics
    ///
    /// Panics if the gas string cannot be parsed. Use [`Gas`]'s `FromStr` impl
    /// for fallible parsing of user input.
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
    /// # Panics
    ///
    /// Panics if the amount string cannot be parsed. Use [`NearToken`]'s `FromStr`
    /// impl for fallible parsing of user input.
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
    ///     .into_action();
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
    /// # Panics
    ///
    /// Panics if the underlying transaction builder already has accumulated
    /// actions, since those would be silently dropped. Use [`finish`](Self::finish)
    /// instead when chaining multiple actions on the same transaction.
    pub fn into_action(self) -> Action {
        assert!(
            self.builder.actions.is_empty(),
            "into_action() discards {} previously accumulated action(s) — \
             use .finish() to keep them in the transaction",
            self.builder.actions.len(),
        );
        self.call.into()
    }

    /// Finish this call and return to the transaction builder.
    ///
    /// This is useful when you need to conditionally add actions to a
    /// transaction, since it gives back the [`TransactionBuilder`] so you can
    /// branch on runtime state before starting the next action.
    pub fn finish(self) -> TransactionBuilder {
        self.builder.add_action(self.call)
    }

    // ========================================================================
    // Chaining methods (delegate to TransactionBuilder after finishing)
    // ========================================================================

    /// Add a pre-built action to the transaction.
    ///
    /// Finishes this function call, then adds the given action.
    /// See [`TransactionBuilder::add_action`] for details.
    pub fn add_action(self, action: impl Into<Action>) -> TransactionBuilder {
        self.finish().add_action(action)
    }

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
        receiver_id: impl TryIntoAccountId,
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
    pub fn delete_account(self, beneficiary_id: impl TryIntoAccountId) -> TransactionBuilder {
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
    pub fn deploy_from_publisher(self, publisher_id: impl TryIntoAccountId) -> TransactionBuilder {
        self.finish().deploy_from_publisher(publisher_id)
    }

    /// Create a NEP-616 deterministic state init action.
    pub fn state_init(
        self,
        state_init: DeterministicAccountStateInit,
        deposit: impl IntoNearToken,
    ) -> TransactionBuilder {
        self.finish().state_init(state_init, deposit)
    }

    /// Override the signer.
    pub fn sign_with(self, signer: impl Signer + 'static) -> TransactionBuilder {
        self.finish().sign_with(signer)
    }

    /// Set the execution wait level.
    pub fn wait_until(self, status: TxExecutionStatus) -> TransactionBuilder {
        self.finish().wait_until(status)
    }

    /// Override the number of nonce retries for this transaction on `InvalidNonce`
    /// errors. `0` means no retries (send once), `1` means one retry, etc.
    pub fn max_nonce_retries(self, retries: u32) -> TransactionBuilder {
        self.finish().max_nonce_retries(retries)
    }

    /// Build and sign a delegate action for meta-transactions (NEP-366).
    ///
    /// This finishes the current function call and then creates a delegate action.
    pub async fn delegate(self, options: DelegateOptions) -> Result<DelegateResult, crate::Error> {
        self.finish().delegate(options).await
    }

    /// Sign the transaction offline without network access.
    ///
    /// See [`TransactionBuilder::sign_offline`] for details.
    pub async fn sign_offline(
        self,
        block_hash: CryptoHash,
        nonce: u64,
    ) -> Result<SignedTransaction, Error> {
        self.finish().sign_offline(block_hash, nonce).await
    }

    /// Sign the transaction without sending it.
    ///
    /// See [`TransactionBuilder::sign`] for details.
    pub async fn sign(self) -> Result<SignedTransaction, Error> {
        self.finish().sign().await
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

    /// Override the number of nonce retries for this transaction on `InvalidNonce`
    /// errors. `0` means no retries (send once), `1` means one retry, etc.
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.builder.max_nonce_retries = retries;
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

            tracing::info!(
                sender = %signer_id,
                receiver = %builder.receiver_id,
                action_count = builder.actions.len(),
                "Sending transaction"
            );

            // Retry loop for transient InvalidTxErrors (nonce conflicts, expired block hash)
            let max_nonce_retries = builder.max_nonce_retries;
            let network = builder.rpc.url().to_string();
            let mut last_error: Option<Error> = None;
            let mut last_ak_nonce: Option<u64> = None;

            for attempt in 0..=max_nonce_retries {
                // Get a signing key atomically for this attempt
                let key = signer.key();
                let public_key = key.public_key().clone();
                let public_key_str = public_key.to_string();

                // Single view_access_key call provides both nonce and block_hash.
                // Uses Finality::Final for block hash stability.
                let access_key = builder
                    .rpc
                    .view_access_key(
                        &signer_id,
                        &public_key,
                        BlockReference::Finality(Finality::Final),
                    )
                    .await?;
                let block_hash = access_key.block_hash;

                // Resolve nonce: use ak_nonce from InvalidNonce error if
                // available, otherwise use the nonce manager (which caches
                // locally after the first fetch).
                let nonce = if let Some(ak_nonce) = last_ak_nonce.take() {
                    nonce_manager().update_and_get_next(
                        &network,
                        signer_id.as_ref(),
                        &public_key_str,
                        ak_nonce,
                    )
                } else {
                    nonce_manager()
                        .get_next_nonce(&network, signer_id.as_ref(), &public_key_str, || async {
                            Ok(access_key.nonce)
                        })
                        .await?
                };

                // Build transaction
                let tx = Transaction::new(
                    signer_id.clone(),
                    public_key.clone(),
                    nonce,
                    builder.receiver_id.clone(),
                    block_hash,
                    builder.actions.clone(),
                );

                // Sign with the key
                let signature = match key.sign(tx.get_hash().as_bytes()).await {
                    Ok(sig) => sig,
                    Err(e) => return Err(Error::Signing(e)),
                };
                let signed_tx = crate::types::SignedTransaction {
                    transaction: tx,
                    signature,
                };

                // Send
                match builder.rpc.send_tx(&signed_tx, builder.wait_until).await {
                    Ok(response) => {
                        let outcome = response.outcome.ok_or_else(|| {
                            Error::InvalidTransaction(format!(
                                "Transaction {} submitted with wait_until={:?} but no execution \
                                 outcome was returned. Use rpc().send_tx() for fire-and-forget \
                                 submission.",
                                response.transaction_hash, builder.wait_until,
                            ))
                        })?;

                        // Inspect outcome status — only InvalidTxError becomes Err.
                        // ActionError means the tx executed (nonce incremented, gas consumed),
                        // so we return Ok(outcome) and let the caller inspect is_failure().
                        use crate::types::{FinalExecutionStatus, TxExecutionError};
                        match outcome.status {
                            FinalExecutionStatus::Failure(TxExecutionError::InvalidTxError(e)) => {
                                return Err(Error::InvalidTx(Box::new(e)));
                            }
                            _ => return Ok(outcome),
                        }
                    }
                    Err(RpcError::InvalidTx(crate::types::InvalidTxError::InvalidNonce {
                        tx_nonce,
                        ak_nonce,
                    })) if attempt < max_nonce_retries => {
                        tracing::warn!(
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
                        if attempt < max_nonce_retries - 1 =>
                    {
                        tracing::warn!(
                            attempt = attempt + 1,
                            "Transaction expired (stale block hash), retrying with fresh block hash"
                        );
                        // Expired tx was rejected before nonce consumption.
                        // Invalidate the nonce cache so the next iteration re-fetches
                        // via view_access_key, which gives both a fresh nonce and block_hash.
                        nonce_manager().invalidate(&network, signer_id.as_ref(), &public_key_str);
                        last_error = Some(Error::InvalidTx(Box::new(
                            crate::types::InvalidTxError::Expired,
                        )));
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Transaction send failed");
                        return Err(e.into());
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a TransactionBuilder for unit tests (no real network needed).
    fn test_builder() -> TransactionBuilder {
        let rpc = Arc::new(RpcClient::new("https://rpc.testnet.near.org"));
        let receiver: AccountId = "contract.testnet".parse().unwrap();
        TransactionBuilder::new(rpc, None, receiver, 0)
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
    fn add_action_works_after_call_builder() {
        let extra_action = Action::transfer(NearToken::from_near(1));

        let builder = test_builder()
            .call("setup")
            .args(serde_json::json!({ "admin": "alice.testnet" }))
            .gas(Gas::from_tgas(50))
            .add_action(extra_action);

        // Should have two actions: the function call from CallBuilder + the transfer
        assert_eq!(builder.actions.len(), 2);
    }

    // FunctionCall tests

    #[test]
    fn function_call_into_action() {
        let call = FunctionCall::new("init")
            .args(serde_json::json!({"owner": "alice.testnet"}))
            .gas(Gas::from_tgas(50))
            .deposit(NearToken::from_near(1));

        let action: Action = call.into();
        match &action {
            Action::FunctionCall(fc) => {
                assert_eq!(fc.method_name, "init");
                assert_eq!(
                    fc.args,
                    serde_json::to_vec(&serde_json::json!({"owner": "alice.testnet"})).unwrap()
                );
                assert_eq!(fc.gas, Gas::from_tgas(50));
                assert_eq!(fc.deposit, NearToken::from_near(1));
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn function_call_defaults() {
        let call = FunctionCall::new("method");
        let action: Action = call.into();
        match &action {
            Action::FunctionCall(fc) => {
                assert_eq!(fc.method_name, "method");
                assert!(fc.args.is_empty());
                assert_eq!(fc.gas, Gas::from_tgas(30));
                assert_eq!(fc.deposit, NearToken::ZERO);
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
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
            .into_action();

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
            .into_action();

        let action2 = test_builder()
            .call("method_b")
            .deposit(NearToken::from_near(1))
            .into_action();

        let builder = test_builder().add_action(action1).add_action(action2);

        assert_eq!(builder.actions.len(), 2);
    }
}
