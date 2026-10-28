//! Transaction sending: [`Tx`], the private send path shared by
//! [`SigningNear`](crate::SigningNear), and pre-signed submission.
//!
//! # Example
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # use near_kit::protocol::Action;
//! # use near_kit::signer::PublicKey;
//! # use near_kit::transaction::FunctionCall;
//! # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
//! // Create a new sub-account with funding, a key and a contract
//! let new_public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
//! let wasm_code = std::fs::read("contract.wasm").expect("failed to read wasm");
//! alice
//!     .transaction(account!("new.alice.testnet"))
//!     .add_action(Action::create_account())
//!     .add_action(Action::transfer(NearToken::from_near(5)))
//!     .add_action(Action::add_full_access_key(new_public_key))
//!     .add_action(Action::deploy_contract(wasm_code))
//!     .add_action(FunctionCall::new("init").args(serde_json::json!({ "owner": "alice.testnet" })))
//!     .await?.result()?;
//! # Ok(())
//! # }
//! ```

use std::fmt;
use std::future::IntoFuture;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

use crate::trace::{self, Instrument};

use crate::error::{Error, RpcError};
use crate::transaction::CodecError;
use crate::types::{
    AccountId, Action, BlockReference, ChainId, CryptoHash, DelegateAction, Finality,
    NonDelegateAction, PublicKey, SignedDelegateAction, SignedTransaction, Transaction,
    VersionedDelegateActionPayload, WaitLevel,
};

use super::near::Near;
use super::nonce_manager::NonceManager;
use super::rpc::RpcClient;
use super::signing::SigningNear;
use crate::types::{Signer, SigningKey};

/// Global nonce manager shared across all signing clients, keyed by chain
/// id, account and public key, so clients for one network share a sequence
/// whatever their RPC URLs. An implementation detail, not exposed to users.
fn nonce_manager() -> &'static NonceManager {
    static NONCE_MANAGER: OnceLock<NonceManager> = OnceLock::new();
    NONCE_MANAGER.get_or_init(NonceManager::new)
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
            Action::UniversalStateInit(_) => out.push_str("universal_state_init"),
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
///
/// Set the fields you need on top of `Default`:
/// `DelegateOptions { block_height_offset: Some(100), ..Default::default() }`.
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

/// A non-empty list of actions for one receiver, with the signer and RPC
/// connection that will sign and send it.
///
/// Keeping this stage private lets every terminal path ([`Tx`],
/// [`SigningNear::sign`], [`SigningNear::sign_delegate`]) share validation,
/// online nonce resolution, and signing without exposing partially prepared
/// state in the public API.
pub(crate) struct PreparedTransaction {
    rpc: Arc<RpcClient>,
    chain_id: ChainId,
    signer: Arc<dyn Signer>,
    receiver_id: AccountId,
    actions: Vec<Action>,
    max_nonce_retries: u32,
}

impl PreparedTransaction {
    /// Validate the action list for one terminal operation.
    pub(crate) fn new(
        near: &Near,
        signer: Arc<dyn Signer>,
        receiver_id: AccountId,
        actions: Vec<Action>,
        empty_error: &'static str,
    ) -> Result<Self, Error> {
        if actions.is_empty() {
            return Err(Error::Validation(empty_error.to_string()));
        }
        // A state init only applies to the account it derives, and a delegate
        // action only to its sender, so the receiver has to be that account.
        // Catch it here rather than on chain, where the relayer pays for it.
        for action in &actions {
            let (what, required) = match action {
                Action::DeterministicStateInit(action) => {
                    ("state init creates", action.derive_account_id())
                }
                Action::UniversalStateInit(action) => {
                    ("state init creates", action.derive_account_id())
                }
                Action::Delegate(action) => ("delegate action is from", action.sender_id().clone()),
                Action::DelegateV2(action) => match &action.delegate_action {
                    VersionedDelegateActionPayload::V2(payload) => {
                        ("delegate action is from", payload.sender_id.clone())
                    }
                },
                _ => continue,
            };
            if required != receiver_id {
                return Err(Error::Validation(format!(
                    "{what} {required}, but the receiver is {receiver_id}; \
                     send the transaction to {required}"
                )));
            }
        }
        Ok(Self {
            rpc: near.rpc.clone(),
            chain_id: near.chain_id().clone(),
            signer,
            receiver_id,
            actions,
            max_nonce_retries: near.max_nonce_retries,
        })
    }

    /// Claim one signing key for the logical operation.
    fn claim_signer(&self) -> (AccountId, SigningKey) {
        (self.signer.account_id().clone(), self.signer.key())
    }

    /// Resolve the next nonce and block hash for one online attempt.
    async fn online_context(
        &self,
        signer_id: &AccountId,
        public_key: &PublicKey,
        nonce_floor: Option<u64>,
    ) -> Result<(u64, CryptoHash), Error> {
        let (chain_nonce, block_height, block_hash) = match self
            .rpc
            .view_access_key(
                signer_id,
                public_key,
                BlockReference::Finality(Finality::Final),
            )
            .await
        {
            Ok(access_key) => (
                access_key.nonce,
                access_key.block_height,
                access_key.block_hash,
            ),
            // A self-signed universal state init installs its own signing key,
            // so the key isn't on chain yet: the nonce comes from the
            // uninitialized account's `bootstrap_nonce` instead.
            Err(error @ RpcError::AccessKeyNotFound { .. })
                if self.bootstraps(signer_id, public_key) =>
            {
                let account = self
                    .rpc
                    .view_account(signer_id, BlockReference::Finality(Finality::Final))
                    .await?;
                let bootstrap_nonce = account.bootstrap_nonce.ok_or(error)?;
                (bootstrap_nonce, account.block_height, account.block_hash)
            }
            Err(error) => return Err(error.into()),
        };
        let nonce = nonce_manager().next(
            &self.chain_id,
            signer_id,
            public_key,
            nonce_floor.unwrap_or(chain_nonce),
            block_height,
        )?;
        Ok((nonce, block_hash))
    }

    /// Whether this transaction bootstraps `signer_id`: it carries the state
    /// init of the signer's own `0u` account, and that state init installs
    /// `public_key`.
    fn bootstraps(&self, signer_id: &AccountId, public_key: &PublicKey) -> bool {
        self.actions.iter().any(|action| match action {
            Action::UniversalStateInit(action) => {
                action.derive_account_id() == *signer_id
                    && action
                        .decode_state_init()
                        .is_ok_and(|state_init| state_init.has_access_key(public_key))
            }
            _ => false,
        })
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

    /// Resolve the nonce and block hash online and sign the transaction.
    pub(crate) async fn sign(self) -> Result<SignedTransaction, Error> {
        let prepared = self;
        let (signer_id, key) = prepared.claim_signer();
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

    /// Sign and send, re-signing with a fresh nonce or block hash on
    /// `InvalidNonce` / `NonceTooLarge` / `Expired` up to `max_nonce_retries`
    /// times.
    pub(crate) async fn send<W: WaitLevel>(self) -> Result<W::Response, Error> {
        let prepared = self;
        // Claim one key for the entire logical send. Retries must keep
        // nonce state and signatures paired with that same access key.
        let (signer_id, key) = prepared.claim_signer();
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
            let mut last_ak_nonce: Option<u64> = None;

            // Every pass returns or retries, and a retry needs
            // `attempt < max_nonce_retries`, so the last allowed attempt
            // returns its own error.
            let mut attempt: u32 = 0;
            loop {
                let (nonce, block_hash) = prepared
                    .online_context(&signer_id, &public_key, last_ak_nonce.take())
                    .await?;

                let tx =
                    prepared.transaction(signer_id.clone(), public_key.clone(), nonce, block_hash);
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
                        return Ok(W::convert(response, &signer_id)?);
                    }
                    Err(RpcError::InvalidTx(crate::types::InvalidTxError::InvalidNonce {
                        tx_nonce: _tx_nonce,
                        ak_nonce,
                    })) if attempt < max_nonce_retries => {
                        trace::debug!(
                            tx_nonce = _tx_nonce,
                            ak_nonce = ak_nonce,
                            attempt = attempt + 1,
                            "Invalid nonce, retrying"
                        );
                        // Store ak_nonce for next iteration to avoid refetching
                        last_ak_nonce = Some(ak_nonce);
                        attempt += 1;
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
                        attempt += 1;
                        continue;
                    }
                    Err(RpcError::InvalidTx(crate::types::InvalidTxError::NonceTooLarge {
                        tx_nonce: _tx_nonce,
                        upper_bound: _upper_bound,
                    })) if attempt < max_nonce_retries => {
                        trace::debug!(
                            tx_nonce = _tx_nonce,
                            upper_bound = _upper_bound,
                            attempt = attempt + 1,
                            "Nonce too large, retrying from the access key's nonce"
                        );
                        // The cached nonce is past what this chain accepts (it
                        // came from another chain with this id), and the
                        // rejection consumed nothing: start again from the view.
                        nonce_manager().forget(&prepared.chain_id, &signer_id, &public_key);
                        attempt += 1;
                        continue;
                    }
                    Err(e) => {
                        // DEBUG, not ERROR: the error is returned to the
                        // caller (see `RpcClient::request`).
                        trace::debug!(error = %e, "Transaction send failed");
                        return Err(e.into());
                    }
                }
            }
        }
        .instrument(span)
        .await
    }

    /// Build and sign a delegate action (NEP-366) from these actions.
    pub(crate) async fn delegate(
        self,
        options: DelegateOptions,
    ) -> Result<SignedDelegateAction, Error> {
        let prepared = self;

        // Verify no nested delegates (of any version)
        for action in &prepared.actions {
            if action.is_delegate() {
                return Err(Error::Validation(
                    "Delegate actions cannot contain nested signed delegate actions".to_string(),
                ));
            }
        }

        let (sender_id, key) = prepared.claim_signer();
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

        Ok(delegate_action.into_signed(signature))
    }
}

// ============================================================================
// Tx
// ============================================================================

/// A transaction being assembled: a receiver, the actions so far, and the
/// [`SigningNear`] that will sign and send it.
///
/// Created by [`SigningNear::transaction`]. Add actions with
/// [`add_action`](Self::add_action), then `.await` it. Awaiting signs with a
/// fresh nonce, sends, retries `InvalidNonce` / `NonceTooLarge` / `Expired`
/// rejections up to the client's
/// [`max_nonce_retries`](crate::SigningNear::max_nonce_retries),
/// and waits for the client's wait level `W`, so it resolves to
/// `W::Response` (a [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome)
/// for the default [`Final`](crate::transaction::Final)). Final is also what
/// reads default to, so a default read after a default write sees it.
///
/// Actions come from the [`Action`] constructors, typed
/// [`FunctionCall`](crate::transaction::FunctionCall)s, or the static
/// constructors `#[near_kit::contract]` generates. An argument-encoding
/// error is kept and returned from `.await`, so building never fails.
///
/// Before signing, `.await` checks the transaction: an empty action list,
/// or a state init (`Action::state_init`, `Action::universal_state_init`)
/// whose derived account isn't the receiver, is an
/// [`Error::Validation`] and nothing is sent.
///
/// To sign without sending, or to build a meta-transaction, pass the same
/// actions to [`SigningNear::sign`] or [`SigningNear::sign_delegate`].
///
/// # Example
///
/// ```rust,no_run
/// # use near_kit::*;
/// # use near_kit::protocol::Action;
/// # use near_kit::transaction::{ExecutedOptimistic, FunctionCall};
/// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
/// alice
///     .transaction(account!("bob.testnet"))
///     .add_action(Action::transfer(NearToken::from_near(1)))
///     .await?.result()?;
///
/// // Several actions in one atomic transaction, returning as soon as it
/// // executes rather than waiting for finality.
/// alice
///     .wait_until::<ExecutedOptimistic>()
///     .transaction(account!("counter.testnet"))
///     .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(10)))
///     .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(10)))
///     .await?.result()?;
/// # Ok(())
/// # }
/// ```
#[must_use = "transactions are not sent unless awaited"]
pub struct Tx<W: WaitLevel = crate::types::Final> {
    pub(crate) sender: SigningNear<W>,
    pub(crate) receiver_id: AccountId,
    pub(crate) actions: Vec<Action>,
    /// The first deferred error; returned from `.await`.
    pub(crate) error: Option<Error>,
}

impl<W: WaitLevel> Tx<W> {
    pub(crate) fn new(sender: SigningNear<W>, receiver_id: AccountId) -> Self {
        Self {
            sender,
            receiver_id,
            actions: Vec::new(),
            error: None,
        }
    }

    /// Append an action.
    ///
    /// Takes an [`Action`] or anything that converts into one, such as a
    /// [`FunctionCall`](crate::transaction::FunctionCall). If its arguments
    /// fail to encode, the [`CodecError`] is kept and returned when the
    /// transaction is awaited; later actions are still accepted but the
    /// transaction is never sent.
    pub fn add_action<A>(mut self, action: A) -> Self
    where
        A: TryInto<Action>,
        A::Error: Into<CodecError>,
    {
        match action.try_into() {
            Ok(action) => self.actions.push(action),
            Err(error) => {
                let error: CodecError = error.into();
                self.defer(error.into());
            }
        }
        self
    }

    /// Keep `error` (unless an earlier one is already kept) to return from
    /// `.await`.
    pub(crate) fn defer(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
}

impl<W: WaitLevel> fmt::Debug for Tx<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tx")
            .field("signer_id", self.sender.account_id())
            .field("receiver_id", &self.receiver_id)
            .field("action_count", &self.actions.len())
            .field("error", &self.error)
            .finish()
    }
}

impl<W: WaitLevel> IntoFuture for Tx<W> {
    type Output = Result<W::Response, Error>;
    type IntoFuture = crate::platform::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if let Some(error) = self.error {
                return Err(error);
            }
            PreparedTransaction::new(
                self.sender.near(),
                self.sender.signer().clone(),
                self.receiver_id,
                self.actions,
                "Transaction must have at least one action",
            )?
            .send::<W>()
            .await
        })
    }
}

// ============================================================================
// SignedTransactionSend
// ============================================================================

/// Awaitable builder for sending a pre-signed transaction.
///
/// Created by [`Near::send_signed`]. The type parameter `W` determines the wait
/// level and response type. Awaiting
/// this builder directly uses [`Final`](crate::transaction::Final), like
/// [`SigningNear`],
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
/// // Final default and returns FinalExecutionOutcome instead.
/// let submitted: SendTxResponse = near.send_signed(signed)
///     .wait_until::<Included>()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[must_use = "signed transactions are not sent unless awaited"]
pub struct SignedTransactionSend<'tx, W: WaitLevel = crate::types::Final> {
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
            Ok(W::convert(response, sender_id)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{self, Write};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::account;
    use crate::client::{BoxFuture, RpcTransport, TransportResponse};
    use crate::transaction::FunctionCall;
    use crate::types::{
        Gas, InMemorySigner, NearToken, RotatingSigner, SecretKey, StateInit, Submitted,
    };

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

    /// A signing client for unit tests whose RPC endpoint is never reached.
    fn test_signing() -> SigningNear {
        crate::Near::custom("http://127.0.0.1:1", "test")
            .build()
            .with_signer(InMemorySigner::new(
                account!("alice.testnet"),
                SecretKey::generate_ed25519(),
            ))
    }

    fn test_tx() -> Tx {
        test_signing().transaction(crate::account!("contract.testnet"))
    }

    #[test]
    fn add_action_appends_in_order() {
        let action = Action::function_call(
            "do_something",
            serde_json::to_vec(&serde_json::json!({ "key": "value" })).unwrap(),
            Gas::from_tgas(30),
            NearToken::ZERO,
        );

        let tx = test_tx()
            .add_action(Action::create_account())
            .add_action(Action::transfer(NearToken::from_near(5)))
            .add_action(action)
            .add_action(FunctionCall::new("init").gas(Gas::from_tgas(50)));

        assert_eq!(tx.receiver_id.as_str(), "contract.testnet");
        assert!(tx.error.is_none());
        assert_eq!(tx.actions.len(), 4);
        assert!(matches!(&tx.actions[0], Action::CreateAccount(_)));
        assert!(matches!(&tx.actions[1], Action::Transfer(_)));
        assert!(
            matches!(&tx.actions[2], Action::FunctionCall(fc) if fc.method_name == "do_something")
        );
        assert!(
            matches!(&tx.actions[3], Action::FunctionCall(fc) if fc.method_name == "init" && fc.gas == Gas::from_tgas(50))
        );
    }

    #[test]
    fn add_action_in_a_loop() {
        let mut tx = test_tx();
        for method in ["step1", "step2", "step3"] {
            tx = tx.add_action(FunctionCall::new(method));
        }
        assert_eq!(tx.actions.len(), 3);
    }

    #[tokio::test]
    async fn encode_errors_are_deferred_until_await() {
        let error = test_tx()
            .add_action(FunctionCall::new("method").args(FailingJson))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Json(_)), "{error:?}");

        let error = test_tx()
            .add_action(FunctionCall::new("method").args_borsh(FailingBorsh))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Borsh(_)), "{error:?}");
    }

    #[tokio::test]
    async fn the_first_deferred_error_wins() {
        let tx = test_tx()
            .add_action(FunctionCall::new("ok"))
            .add_action(FunctionCall::new("bad").args(FailingJson))
            .add_action(FunctionCall::new("worse").args_borsh(FailingBorsh))
            .add_action(Action::transfer(NearToken::from_near(1)));
        assert_eq!(tx.actions.len(), 2, "later valid actions are still kept");
        assert!(matches!(tx.await.unwrap_err(), Error::Json(_)));
    }

    #[tokio::test]
    async fn empty_action_lists_are_rejected_before_any_rpc() {
        let error = test_tx().await.unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");

        let signing = test_signing();
        let receiver = crate::account!("contract.testnet");
        let error = signing.sign(&receiver, Vec::new()).await.unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");
        let error = signing
            .sign_delegate(&receiver, Vec::new(), Default::default())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");
    }

    #[tokio::test]
    async fn relayed_delegate_to_another_receiver_is_rejected_before_any_rpc() {
        // nearcore fails a delegate whose sender isn't the transaction's
        // receiver (DelegateActionSenderDoesNotMatchTxReceiver), after the
        // relayer has paid for it. The old builder set the receiver itself.
        let key = SecretKey::generate_ed25519();
        let delegate = DelegateAction {
            sender_id: "sender.testnet".parse().unwrap(),
            receiver_id: "guestbook.testnet".parse().unwrap(),
            actions: vec![
                NonDelegateAction::from_action(Action::transfer(NearToken::ZERO)).unwrap(),
            ],
            nonce: 1,
            max_block_height: 100,
            public_key: key.public_key(),
        };
        let signature = key.sign(delegate.get_hash().as_bytes());
        let signed_delegate = delegate.into_signed(signature);

        let error = test_signing()
            .transaction(crate::account!("guestbook.testnet"))
            .add_action(Action::delegate(signed_delegate.clone()))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, Error::Validation(message)
                if message.contains("sender.testnet") && message.contains("guestbook.testnet")),
            "{error:?}"
        );

        let error = test_signing()
            .sign(
                &crate::account!("guestbook.testnet"),
                vec![Action::delegate(signed_delegate)],
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");
    }

    #[tokio::test]
    async fn delegate_rejects_nested_delegate_actions() {
        let key = SecretKey::generate_ed25519();
        let delegate = DelegateAction {
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
        let signed_delegate = delegate.into_signed(signature);

        let error = test_signing()
            .sign_delegate(
                signed_delegate.sender_id(),
                vec![Action::delegate(signed_delegate.clone())],
                Default::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");
    }

    #[tokio::test]
    async fn state_init_to_another_receiver_is_rejected_before_any_rpc() {
        let state_init = StateInit::from(crate::protocol::StateInitV1::code(CryptoHash::ZERO));
        let derived = state_init.derive_account_id();
        let signing = test_signing();
        let wrong = crate::account!("contract.testnet");

        let error = signing
            .transaction(&wrong)
            .add_action(Action::state_init(state_init.clone(), NearToken::ZERO))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, Error::Validation(message)
                if message.contains(derived.as_str()) && message.contains("contract.testnet")),
            "{error:?}"
        );

        let universal = crate::protocol::UniversalStateInitV1::new()
            .with_access_key(SecretKey::generate_ed25519().public_key());
        let error = signing
            .sign(
                &wrong,
                vec![Action::universal_state_init(universal, NearToken::ZERO)],
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");

        let error = signing
            .sign_delegate(
                &wrong,
                vec![Action::state_init(state_init, NearToken::ZERO)],
                Default::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(_)), "{error:?}");
    }

    #[test]
    fn state_init_actions_name_their_receiver() {
        // The receiver of a state init is the account it derives; the old
        // builder rewrote the receiver, now the caller passes it.
        let state_init = StateInit::from(crate::protocol::StateInitV1::code(CryptoHash::ZERO));
        let tx = test_signing()
            .transaction(state_init.derive_account_id())
            .add_action(Action::state_init(state_init.clone(), NearToken::ZERO));
        assert_eq!(tx.receiver_id, state_init.derive_account_id());

        let universal = crate::protocol::UniversalStateInit::from(
            crate::protocol::UniversalStateInitV1::new()
                .with_access_key(crate::types::PublicKeyHandle::MlDsa65Hash([0x11; 32])),
        );
        // nearcore 2.14-release `test_derive_universal_account_id` key-only vector.
        assert_eq!(
            universal.derive_account_id().as_str(),
            "0ux8te7g99f9kqzdtp9h4qnwt9aczpgayymmtbdc50w199rcw3at1g"
        );
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

    /// An uninitialized `0u` account: every access key is unknown, and
    /// `view_account` reports a `bootstrap_nonce` of 100000000.
    #[derive(Default)]
    struct UninitializedUniversalTransport {
        sent: Mutex<Vec<String>>,
    }

    impl RpcTransport for UninitializedUniversalTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let response = match request["method"].as_str().unwrap() {
                "EXPERIMENTAL_view_access_key" => serde_json::json!({
                    "error": {
                        "name": "HANDLER_ERROR",
                        "cause": {
                            "name": "UNKNOWN_ACCESS_KEY",
                            "info": {
                                "public_key": request["params"]["public_key"],
                                "block_height": 100,
                                "block_hash": "11111111111111111111111111111111",
                            },
                        },
                        "code": -32000,
                        "message": "Server error",
                    },
                }),
                "EXPERIMENTAL_view_account" => serde_json::json!({
                    "result": {
                        "amount": "1000000000000000000000000",
                        "locked": "0",
                        "code_hash": "11111111111111111111111111111111",
                        "storage_usage": 100,
                        "state": "uninitialized",
                        "bootstrap_nonce": 100000000,
                        "block_height": 100,
                        "block_hash": CryptoHash::from_bytes([2; 32]),
                    },
                }),
                "send_tx" => {
                    let signed = request["params"]["signed_tx_base64"].as_str().unwrap();
                    self.sent.lock().unwrap().push(signed.to_string());
                    serde_json::json!({ "result": { "final_execution_status": "NONE" } })
                }
                other => panic!("unexpected RPC method {other}"),
            };
            let mut response = response;
            response["jsonrpc"] = "2.0".into();
            response["id"] = 0.into();
            let body = serde_json::to_vec(&response).unwrap();
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    #[tokio::test]
    async fn self_signed_universal_state_init_uses_the_bootstrap_nonce() {
        let key = SecretKey::generate_ed25519();
        let state_init =
            crate::protocol::UniversalStateInitV1::new().with_access_key(key.public_key());
        let account_id =
            crate::protocol::UniversalStateInit::from(state_init.clone()).derive_account_id();
        let transport = Arc::new(UninitializedUniversalTransport::default());
        let near = crate::Near::custom("http://bootstrap.invalid", "test")
            .transport(transport.clone())
            .build()
            .with_signer(InMemorySigner::new(account_id.clone(), key));

        near.wait_until::<Submitted>()
            .transaction(&account_id)
            .add_action(Action::universal_state_init(state_init, NearToken::ZERO))
            .await
            .expect("bootstrap is sent");

        let sent = transport.sent.lock().unwrap();
        let tx = SignedTransaction::from_base64(&sent[0])
            .unwrap()
            .transaction;
        assert_eq!(tx.signer_id, account_id);
        assert_eq!(tx.receiver_id, account_id);
        assert_eq!(tx.nonce, 100_000_001);
        assert_eq!(
            tx.block_hash,
            CryptoHash::from_bytes([2; 32]),
            "block hash comes from the account view"
        );
    }

    #[tokio::test]
    async fn state_init_without_the_signing_key_keeps_the_access_key_error() {
        let key = SecretKey::generate_ed25519();
        let other = SecretKey::generate_ed25519().public_key();
        let state_init = crate::protocol::UniversalStateInitV1::new().with_access_key(other);
        let account_id =
            crate::protocol::UniversalStateInit::from(state_init.clone()).derive_account_id();
        let near = crate::Near::custom("http://bootstrap.invalid", "test")
            .transport(UninitializedUniversalTransport::default())
            .build()
            .with_signer(InMemorySigner::new(account_id.clone(), key));

        let error = near
            .sign(
                &account_id,
                vec![Action::universal_state_init(state_init, NearToken::ZERO)],
            )
            .await
            .expect_err("the signing key is not installed by the state init");
        assert!(
            matches!(&error, Error::Rpc(e) if matches!(**e, RpcError::AccessKeyNotFound { .. })),
            "{error:?}"
        );
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
    async fn send_refreshes_nonce_and_resigns_on_invalid_nonce() {
        // First send_tx is rejected with InvalidNonce (the key's real nonce is
        // 20); the second is accepted.
        let transport = NonceRetryTransport::new(vec![invalid_nonce_body(6, 20), accepted_body()]);
        let keys = vec![SecretKey::generate_ed25519(), SecretKey::generate_ed25519()];
        let signer = RotatingSigner::new(account!("alice.testnet"), keys).unwrap();
        // Default RetryConfig on purpose: the raw RPC layer must not add
        // identical resends of the rejected payload before this loop runs.
        let near = crate::Near::custom("http://mock.invalid", "test")
            .transport(transport.clone())
            .build()
            .with_signer(signer);

        near.wait_until::<Submitted>()
            .transaction(crate::account!("bob.testnet"))
            .add_action(Action::transfer(NearToken::from_near(1)))
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
    async fn tx_sender_send_uses_the_same_retrying_path() {
        let transport = NonceRetryTransport::new(vec![invalid_nonce_body(6, 20), accepted_body()]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = crate::Near::custom("http://tx-sender.invalid", "test")
            .transport(transport.clone())
            .build()
            .with_signer(signer)
            .wait_until::<Submitted>();

        let response = crate::TxSender::send(
            &near,
            crate::account!("bob.testnet"),
            vec![Action::transfer(NearToken::from_near(1))],
        )
        .await
        .expect("second attempt is accepted");

        assert_eq!(response.sender_id.as_str(), "alice.testnet");
        assert_eq!(transport.sent.lock().unwrap().len(), 2);
        assert_eq!(crate::TxSender::account_id(&near).as_str(), "alice.testnet");
    }

    #[tokio::test]
    async fn expired_retries_honor_configured_maximum() {
        let transport =
            NonceRetryTransport::new(vec![expired_body(), expired_body(), accepted_body()]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = crate::Near::custom("http://expired-retry.invalid", "test")
            .transport(transport.clone())
            .max_nonce_retries(2)
            .build()
            .with_signer(signer);

        near.wait_until::<Submitted>()
            .transaction(crate::account!("bob.testnet"))
            .add_action(Action::transfer(NearToken::from_near(1)))
            .await
            .expect("two configured retries should allow the third send");

        assert_eq!(transport.sent.lock().unwrap().len(), 3);
        assert_eq!(transport.access_key_queries.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn expired_with_zero_retries_sends_once() {
        let transport = NonceRetryTransport::new(vec![expired_body()]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = crate::Near::custom("http://no-expired-retry.invalid", "test")
            .transport(transport.clone())
            .max_nonce_retries(0)
            .build()
            .with_signer(signer);

        let error = near
            .wait_until::<Submitted>()
            .transaction(crate::account!("bob.testnet"))
            .add_action(Action::transfer(NearToken::from_near(1)))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::InvalidTx(error) if matches!(*error, crate::types::InvalidTxError::Expired)
        ));
        assert_eq!(transport.sent.lock().unwrap().len(), 1);
    }

    // ========================================================================
    // Nonce cache: key, reset, bootstrap and delegate nonces (mock transport)
    // ========================================================================

    /// Transport for nonce tests: answers `EXPERIMENTAL_view_access_key` from a
    /// script of `(nonce, block_height)` views (the last one repeats), answers
    /// `send_tx` from a script of responses (accepting once it runs out), and
    /// records every signed payload.
    struct ScriptedNonceTransport {
        views: Mutex<VecDeque<(u64, u64)>>,
        send_tx_responses: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<String>>,
    }

    impl ScriptedNonceTransport {
        fn new(views: Vec<(u64, u64)>, send_tx_responses: Vec<Vec<u8>>) -> Arc<Self> {
            assert!(!views.is_empty());
            Arc::new(Self {
                views: Mutex::new(views.into()),
                send_tx_responses: Mutex::new(send_tx_responses.into()),
                sent: Mutex::new(Vec::new()),
            })
        }

        fn sent_nonces(&self) -> Vec<u64> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|tx| {
                    SignedTransaction::from_base64(tx)
                        .unwrap()
                        .transaction
                        .nonce
                })
                .collect()
        }
    }

    impl RpcTransport for ScriptedNonceTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let body = match request["method"].as_str().unwrap() {
                "EXPERIMENTAL_view_access_key" => {
                    let (nonce, block_height) = {
                        let mut views = self.views.lock().unwrap();
                        if views.len() > 1 {
                            views.pop_front().unwrap()
                        } else {
                            views[0]
                        }
                    };
                    serde_json::to_vec(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 0,
                        "result": {
                            "nonce": nonce,
                            "permission": "FullAccess",
                            "block_height": block_height,
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
                        .unwrap_or_else(accepted_body)
                }
                other => panic!("unexpected RPC method {other}"),
            };
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    fn nonce_too_large_body(tx_nonce: u64, upper_bound: u64) -> Vec<u8> {
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
                            "NonceTooLarge": { "tx_nonce": tx_nonce, "upper_bound": upper_bound }
                        }
                    }
                },
            },
        }))
        .unwrap()
    }

    fn signing_via(
        url: &str,
        chain_id: &str,
        transport: Arc<ScriptedNonceTransport>,
        signer: InMemorySigner,
    ) -> SigningNear {
        crate::Near::custom(url, chain_id)
            .transport(transport)
            .build()
            .with_signer(signer)
    }

    fn transfer() -> Vec<Action> {
        vec![Action::transfer(NearToken::from_yoctonear(1))]
    }

    #[tokio::test]
    async fn clients_for_one_chain_share_nonces_across_url_spellings() {
        // Two spellings of one endpoint used to get separate cache entries,
        // so both clients signed nonce 6: byte-identical transactions for
        // identical actions, of which the node runs one.
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let via_ip = signing_via(
            "http://127.0.0.1:3030",
            "url-spellings",
            transport.clone(),
            signer.clone(),
        );
        let via_name = signing_via("http://localhost:3030", "url-spellings", transport, signer);

        let first = via_ip
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        let second = via_name
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        assert_eq!(first.transaction.nonce, 6);
        assert_eq!(second.transaction.nonce, 7);
    }

    #[tokio::test]
    async fn cached_nonce_above_the_chain_bound_resets_to_the_view() {
        // One endpoint first serves a chain at height 1000, then another at
        // height 122 (a restarted node on a reused port). A nonce cached from
        // the first is at or above the second's `height * 1e6` bound, so it
        // can never land there; the entry has to fall back to the view.
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let tall = ScriptedNonceTransport::new(vec![(999_000_000, 1000)], vec![]);
        let short = ScriptedNonceTransport::new(vec![(121_000_000, 122)], vec![]);
        let url = "http://reused-port.invalid";

        let first = signing_via(url, "reused-port", tall, signer.clone())
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        assert_eq!(first.transaction.nonce, 999_000_001);

        let second = signing_via(url, "reused-port", short, signer)
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        assert_eq!(second.transaction.nonce, 121_000_001);
    }

    #[tokio::test]
    async fn nonce_too_large_resets_the_entry_and_retries() {
        // Three signed-but-unsent transactions leave the cache at 8. The node
        // then rejects 9 as too large: the entry can't be trusted, so the
        // retry starts again from the view.
        let transport = ScriptedNonceTransport::new(
            vec![(5, 100)],
            vec![nonce_too_large_body(9, 9), accepted_body()],
        );
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via(
            "http://too-large.invalid",
            "too-large",
            transport.clone(),
            signer,
        );
        for _ in 0..3 {
            near.sign(account!("bob.testnet"), transfer())
                .await
                .unwrap();
        }

        near.wait_until::<Submitted>()
            .transaction(account!("bob.testnet"))
            .add_action(Action::transfer(NearToken::from_yoctonear(1)))
            .await
            .expect("the retry from the view is accepted");
        assert_eq!(transport.sent_nonces(), vec![9, 6]);
    }

    #[tokio::test]
    async fn server_nonce_at_u64_max_is_an_error_not_a_panic() {
        let transport = ScriptedNonceTransport::new(vec![(u64::MAX, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via("http://max-nonce.invalid", "max-nonce", transport, signer);

        for _ in 0..2 {
            let error = near
                .sign(account!("bob.testnet"), transfer())
                .await
                .expect_err("no nonce follows u64::MAX");
            assert!(
                matches!(&error, Error::Rpc(e) if matches!(**e, RpcError::InvalidResponse(_))),
                "{error:?}"
            );
        }

        // The shared cache still works for every other key.
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let signed = signing_via("http://max-nonce.invalid", "max-nonce", transport, signer)
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        assert_eq!(signed.transaction.nonce, 6);
    }
}
