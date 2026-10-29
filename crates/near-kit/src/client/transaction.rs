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
use super::nonce_manager::{Chain, GenesisHash, NonceManager, no_next_nonce};
use super::rpc::RpcClient;
use super::signing::SigningNear;
use crate::types::{Signer, SigningKey};

/// Global nonce manager shared across all signing clients, keyed by the
/// chain's genesis hash, account and public key, so clients for one chain
/// share a sequence whatever their RPC URLs or chain ids, and chains that
/// share a chain id don't. An implementation detail, not exposed to users.
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

    /// Override nonce to use for the delegate action. If omitted, the next
    /// nonce for the signing key comes from the same in-process sequence as
    /// [`SigningNear::sign`] and sent transactions: the access key's nonce + 1,
    /// or higher while transactions or delegate actions this process signed
    /// with that key haven't landed yet.
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
    genesis: Arc<GenesisHash>,
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
            genesis: near.genesis.clone(),
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

    /// The chain this client's nonces are cached for.
    async fn chain(&self) -> Result<Chain, Error> {
        self.genesis.chain(&self.rpc, &self.chain_id).await
    }

    /// Resolve the next nonce and block hash for one online attempt, and the
    /// chain the nonce was cached for (`None` for a bootstrap, which skips
    /// the cache).
    ///
    /// `nonce_floor` is the access key nonce a rejected attempt reported; the
    /// fresh view counts too when it is already higher.
    async fn online_context(
        &self,
        signer_id: &AccountId,
        public_key: &PublicKey,
        nonce_floor: Option<u64>,
    ) -> Result<(u64, CryptoHash, Option<Chain>), Error> {
        let (chain_nonce, block_hash) = match self
            .rpc
            .view_access_key(
                signer_id,
                public_key,
                BlockReference::Finality(Finality::Final),
            )
            .await
        {
            Ok(access_key) => (access_key.nonce, access_key.block_hash),
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
                // nearcore checks a self-signed bootstrap strictly: only
                // `bootstrap_nonce + 1` is accepted. So neither the cache (a
                // nonce handed out earlier and never sent would push past it)
                // nor a rejection's floor applies; this view is the authority.
                // Once the state init lands, the installed key starts from a
                // higher nonce, so later sends need no cache entry from here.
                let nonce = bootstrap_nonce
                    .checked_add(1)
                    .ok_or_else(|| no_next_nonce(bootstrap_nonce))?;
                return Ok((nonce, account.block_hash, None));
            }
            Err(error) => return Err(error.into()),
        };
        let chain = self.chain().await?;
        let nonce = nonce_manager().next(
            &chain,
            signer_id,
            public_key,
            nonce_floor.map_or(chain_nonce, |floor| floor.max(chain_nonce)),
        )?;
        Ok((nonce, block_hash, Some(chain)))
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

    async fn sign_transaction(
        transaction: Transaction,
        key: &SigningKey,
    ) -> Result<(SignedTransaction, CryptoHash), Error> {
        let tx_hash = transaction.get_hash();
        let signature = key.sign(tx_hash.as_bytes()).await?;
        Ok((transaction.into_signed(signature), tx_hash))
    }

    /// Resolve the nonce and block hash online and sign the transaction.
    pub(crate) async fn sign(self) -> Result<SignedTransaction, Error> {
        let (signer_id, key) = self.claim_signer();
        let public_key = key.public_key().clone();

        let span = trace::info_span!(
            "sign_transaction",
            sender = %signer_id,
            receiver = %self.receiver_id,
            action_count = self.actions.len(),
            actions = %actions_summary(&self.actions),
            method = trace::field::Empty,
            gas = trace::field::Empty,
            deposit = trace::field::Empty,
        );

        async move {
            #[cfg(feature = "tracing")]
            record_function_call_fields(&self.actions);

            let (nonce, block_hash, _) = self.online_context(&signer_id, &public_key, None).await?;
            let tx = self.transaction(signer_id, public_key, nonce, block_hash);
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
                let (nonce, block_hash, chain) = prepared
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
                        //   plus the outcome, which the node returns from
                        //   IncludedFinal and possibly at Included).
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
                        // The next attempt still fetches the access key (for a
                        // fresh block hash); the reported nonce is a floor in
                        // case that view lags behind the node that rejected us.
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
                        tx_nonce,
                        upper_bound: _upper_bound,
                    })) if attempt < max_nonce_retries => {
                        trace::debug!(
                            tx_nonce = tx_nonce,
                            upper_bound = _upper_bound,
                            attempt = attempt + 1,
                            "Nonce too large, retrying from the access key's nonce"
                        );
                        // The cached nonce is past what this chain accepts (it
                        // was cached by chain id, for a backend that can't
                        // report its genesis hash, so it may come from another
                        // chain with this id), and the rejection consumed
                        // nothing: start again from the view, unless a
                        // concurrent send's rejection already did. Reset the
                        // entry the nonce was drawn from.
                        if let Some(chain) = &chain {
                            nonce_manager().forget_rejected(
                                chain,
                                &signer_id,
                                &public_key,
                                tx_nonce,
                            );
                        }
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
        if self.actions.iter().any(Action::is_delegate) {
            return Err(Error::Validation(
                "Delegate actions cannot contain nested signed delegate actions".to_string(),
            ));
        }

        let (sender_id, key) = self.claim_signer();
        let public_key = key.public_key().clone();

        // Draw from the same sequence as transactions, so delegate actions
        // signed before relaying, or while a transaction is pending, don't
        // share a nonce.
        //
        // Unlike a send, which retries an `InvalidNonce` rejection with a
        // fresh nonce, a delegate action's nonce is fixed once the user signs
        // it. So read the key at `Optimistic`: a final view can trail a nonce
        // another process already used, and the user would have to sign again.
        let nonce = match options.nonce {
            Some(nonce) => nonce,
            None => {
                let access_key = self
                    .rpc
                    .view_access_key(
                        &sender_id,
                        &public_key,
                        BlockReference::Finality(Finality::Optimistic),
                    )
                    .await?;
                nonce_manager().next(
                    &self.chain().await?,
                    &sender_id,
                    &public_key,
                    access_key.nonce,
                )?
            }
        };

        // Get max block height
        let max_block_height = if let Some(h) = options.max_block_height {
            h
        } else {
            let status = self.rpc.status().await?;
            let offset = options.block_height_offset.unwrap_or(200);
            status.sync_info.latest_block_height + offset
        };

        let PreparedTransaction {
            receiver_id,
            actions,
            ..
        } = self;

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
/// a state init (`Action::state_init`, `Action::universal_state_init`)
/// whose derived account isn't the receiver, or a delegate action
/// (`Action::delegate`, `Action::delegate_v2`) sent to anyone but its
/// sender, is an [`Error::Validation`] and nothing is sent. A relayer sends
/// a delegate action to `signed.sender_id()`.
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
            Err(error) => self.defer(Into::<CodecError>::into(error).into()),
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
    type IntoFuture = super::transport::BoxFuture<'static, Self::Output>;

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
    type IntoFuture = super::transport::BoxFuture<'tx, Self::Output>;

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
    use crate::client::test_rpc::{invalid_tx_body, rpc_error, rpc_result};
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

    /// A delegate action from `sender.testnet` to `receiver`, signed by a
    /// fresh key.
    fn signed_delegate(receiver: &str) -> SignedDelegateAction {
        let key = SecretKey::generate_ed25519();
        let delegate = DelegateAction {
            sender_id: "sender.testnet".parse().unwrap(),
            receiver_id: receiver.parse().unwrap(),
            actions: vec![
                NonDelegateAction::from_action(Action::transfer(NearToken::ZERO)).unwrap(),
            ],
            nonce: 1,
            max_block_height: 100,
            public_key: key.public_key(),
        };
        let signature = key.sign(delegate.get_hash().as_bytes());
        delegate.into_signed(signature)
    }

    #[tokio::test]
    async fn relayed_delegate_to_another_receiver_is_rejected_before_any_rpc() {
        // nearcore fails a delegate whose sender isn't the transaction's
        // receiver (DelegateActionSenderDoesNotMatchTxReceiver), after the
        // relayer has paid for it. The old builder set the receiver itself.
        let signed_delegate = signed_delegate("guestbook.testnet");

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
        let signed_delegate = signed_delegate("receiver.testnet");

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

    // ========================================================================
    // Nonce refresh + re-sign on InvalidNonce (mock transport)
    // ========================================================================

    /// An uninitialized `0u` account: every access key is unknown, and
    /// `view_account` reports a `bootstrap_nonce` of 100000000.
    #[derive(Default)]
    struct UninitializedUniversalTransport {
        sent: Mutex<Vec<String>>,
        /// Scripted `send_tx` responses; accepted once these run out.
        send_tx_responses: Mutex<VecDeque<Vec<u8>>>,
    }

    impl RpcTransport for UninitializedUniversalTransport {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let body = match request["method"].as_str().unwrap() {
                "EXPERIMENTAL_view_access_key" => rpc_error(
                    Some("UNKNOWN_ACCESS_KEY"),
                    serde_json::json!({
                        "public_key": request["params"]["public_key"],
                        "block_height": 100,
                        "block_hash": "11111111111111111111111111111111",
                    }),
                    serde_json::Value::Null,
                ),
                "EXPERIMENTAL_view_account" => rpc_result(serde_json::json!({
                    "amount": "1000000000000000000000000",
                    "locked": "0",
                    "code_hash": "11111111111111111111111111111111",
                    "storage_usage": 100,
                    "state": "uninitialized",
                    "bootstrap_nonce": 100000000,
                    "block_height": 100,
                    "block_hash": CryptoHash::from_bytes([2; 32]),
                })),
                "send_tx" => {
                    let signed = request["params"]["signed_tx_base64"].as_str().unwrap();
                    self.sent.lock().unwrap().push(signed.to_string());
                    let scripted = self.send_tx_responses.lock().unwrap().pop_front();
                    scripted.unwrap_or_else(accepted_body)
                }
                other => panic!("unexpected RPC method {other}"),
            };
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

    /// A node's answer to a method it doesn't serve.
    fn method_not_found_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": { "code": -32601, "message": "Method not found" },
        }))
        .unwrap()
    }

    fn status_body(genesis_hash: CryptoHash) -> Vec<u8> {
        rpc_result(serde_json::json!({
            "protocol_version": 83,
            "latest_protocol_version": 83,
            "chain_id": "mock",
            "genesis_hash": genesis_hash,
            "sync_info": {
                "latest_block_hash": "11111111111111111111111111111111",
                "latest_block_height": 100,
                "latest_block_time": "2026-01-01T00:00:00Z",
                "syncing": false,
            },
            "version": { "version": "2.13.0", "build": "mock" },
        }))
    }

    /// A genesis hash no other test's chain has: the nonce cache is global.
    fn unique_genesis() -> CryptoHash {
        static CHAINS: AtomicUsize = AtomicUsize::new(0);
        CryptoHash::hash(&CHAINS.fetch_add(1, Ordering::Relaxed).to_le_bytes())
    }

    fn invalid_nonce_body(tx_nonce: u64, ak_nonce: u64) -> Vec<u8> {
        invalid_tx_body(serde_json::json!({
            "InvalidNonce": { "tx_nonce": tx_nonce, "ak_nonce": ak_nonce }
        }))
    }

    fn accepted_body() -> Vec<u8> {
        rpc_result(serde_json::json!({ "final_execution_status": "NONE" }))
    }

    fn expired_body() -> Vec<u8> {
        invalid_tx_body("Expired".into())
    }

    #[tokio::test]
    async fn send_refreshes_nonce_and_resigns_on_invalid_nonce() {
        // First send_tx is rejected with InvalidNonce (the key's real nonce is
        // 20); the second is accepted.
        let transport = ScriptedNonceTransport::new(
            vec![(5, 100)],
            vec![invalid_nonce_body(6, 20), accepted_body()],
        );
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
            transport.view_finalities.lock().unwrap().len(),
            2,
            "each attempt re-fetches the access key for a fresh block hash"
        );
    }

    #[tokio::test]
    async fn tx_sender_send_uses_the_same_retrying_path() {
        let transport = ScriptedNonceTransport::new(
            vec![(5, 100)],
            vec![invalid_nonce_body(6, 20), accepted_body()],
        );
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
        let transport = ScriptedNonceTransport::new(
            vec![(5, 100)],
            vec![expired_body(), expired_body(), accepted_body()],
        );
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
        assert_eq!(transport.view_finalities.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn expired_with_zero_retries_sends_once() {
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![expired_body()]);
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
    /// `send_tx` from a script of responses (accepting once it runs out),
    /// reports a genesis hash from `status`, and records every signed payload.
    struct ScriptedNonceTransport {
        views: Mutex<VecDeque<(u64, u64)>>,
        send_tx_responses: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<String>>,
        /// The `finality` of each access key view, in order.
        view_finalities: Mutex<Vec<serde_json::Value>>,
        genesis_hash: CryptoHash,
        /// Replies to the first `status` calls; then the genesis hash.
        status_replies: Mutex<VecDeque<Vec<u8>>>,
        status_calls: AtomicUsize,
    }

    impl ScriptedNonceTransport {
        /// A node of a chain no other test uses.
        fn new(views: Vec<(u64, u64)>, send_tx_responses: Vec<Vec<u8>>) -> Arc<Self> {
            Self::on_chain(unique_genesis(), views, send_tx_responses)
        }

        /// A node of the chain with `genesis_hash`.
        fn on_chain(
            genesis_hash: CryptoHash,
            views: Vec<(u64, u64)>,
            send_tx_responses: Vec<Vec<u8>>,
        ) -> Arc<Self> {
            assert!(!views.is_empty());
            Arc::new(Self {
                views: Mutex::new(views.into()),
                send_tx_responses: Mutex::new(send_tx_responses.into()),
                sent: Mutex::new(Vec::new()),
                view_finalities: Mutex::new(Vec::new()),
                genesis_hash,
                status_replies: Mutex::new(VecDeque::new()),
                status_calls: AtomicUsize::new(0),
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
                    self.view_finalities
                        .lock()
                        .unwrap()
                        .push(request["params"]["finality"].clone());
                    let (nonce, block_height) = {
                        let mut views = self.views.lock().unwrap();
                        if views.len() > 1 {
                            views.pop_front().unwrap()
                        } else {
                            views[0]
                        }
                    };
                    rpc_result(serde_json::json!({
                        "nonce": nonce,
                        "permission": "FullAccess",
                        "block_height": block_height,
                        "block_hash": "11111111111111111111111111111111",
                    }))
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
                "status" => {
                    self.status_calls.fetch_add(1, Ordering::SeqCst);
                    let reply = self.status_replies.lock().unwrap().pop_front();
                    reply.unwrap_or_else(|| status_body(self.genesis_hash))
                }
                other => panic!("unexpected RPC method {other}"),
            };
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    fn nonce_too_large_body(tx_nonce: u64, upper_bound: u64) -> Vec<u8> {
        invalid_tx_body(serde_json::json!({
            "NonceTooLarge": { "tx_nonce": tx_nonce, "upper_bound": upper_bound }
        }))
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
        // identical actions, of which the node runs one. Both nodes report
        // one genesis, so both clients draw from one sequence, even under
        // different chain ids.
        let genesis = unique_genesis();
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let via_ip = signing_via(
            "http://127.0.0.1:3030",
            "url-spellings",
            ScriptedNonceTransport::on_chain(genesis, vec![(5, 100)], vec![]),
            signer.clone(),
        );
        let via_name = signing_via(
            "http://localhost:3030",
            "url-spellings-too",
            ScriptedNonceTransport::on_chain(genesis, vec![(5, 100)], vec![]),
            signer,
        );

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
    async fn a_restarted_node_on_a_reused_url_is_another_chain() {
        // One endpoint first serves a chain at height 1000, then another at
        // height 122 (a restarted node on a reused port, same chain id). The
        // second has another genesis, so the first's nonces don't carry over.
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

    /// A node error, which a client with retries off doesn't retry.
    fn internal_error_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "error": { "code": -32603, "message": "Internal error" },
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn a_failed_status_fails_the_draw_and_is_asked_again() {
        // Drawing by chain id while another client on this chain draws by
        // genesis hash could hand a nonce out twice, so there is no draw.
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        transport
            .status_replies
            .lock()
            .unwrap()
            .push_back(internal_error_body());
        let near = crate::Near::custom("http://status-fails.invalid", "status-fails")
            .transport(transport.clone())
            .retry_config(crate::rpc::RetryConfig::none())
            .build()
            .with_signer(InMemorySigner::new(
                account!("alice.testnet"),
                SecretKey::generate_ed25519(),
            ));

        let error = near
            .sign(account!("bob.testnet"), transfer())
            .await
            .expect_err("no genesis hash, no nonce");
        assert!(
            matches!(&error, Error::Rpc(e) if matches!(**e, RpcError::Rpc { code: -32603, .. })),
            "{error:?}"
        );
        // Asked again: the failed draw handed out no nonce.
        let delegate = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        assert_eq!(transport.status_calls.load(Ordering::SeqCst), 2);
        assert_eq!(delegate.delegate_action.nonce, 6);

        // Kept from then on, by clones too.
        let signed = near
            .clone()
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        assert_eq!(signed.transaction.nonce, 7);
        assert_eq!(transport.status_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_backend_without_status_shares_nonces_by_chain_id() {
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let without_status = |url| {
            let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
            transport
                .status_replies
                .lock()
                .unwrap()
                .push_back(method_not_found_body());
            (
                signing_via(url, "no-status", transport.clone(), signer.clone()),
                transport,
            )
        };
        let (first, first_transport) = without_status("http://no-status-a.invalid");
        let (second, _) = without_status("http://no-status-b.invalid");

        let mut nonces = Vec::new();
        for near in [&first, &second, &first] {
            nonces.push(
                near.sign(account!("bob.testnet"), transfer())
                    .await
                    .unwrap()
                    .transaction
                    .nonce,
            );
        }
        // One sequence by chain id, and the answer is kept: `status` isn't
        // asked again, though the transport would now report a genesis hash.
        assert_eq!(nonces, vec![6, 7, 8]);
        assert_eq!(first_transport.status_calls.load(Ordering::SeqCst), 1);
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
    async fn invalid_nonce_retry_uses_a_fresher_view() {
        // The error reports 20, but by the retry the view already shows 30
        // (other senders landed meanwhile): 21 is certain to fail again.
        let transport = ScriptedNonceTransport::new(
            vec![(5, 100), (30, 101)],
            vec![invalid_nonce_body(6, 20), accepted_body()],
        );
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        signing_via(
            "http://fresher.invalid",
            "fresher-view",
            transport.clone(),
            signer,
        )
        .wait_until::<Submitted>()
        .transaction(account!("bob.testnet"))
        .add_action(Action::transfer(NearToken::from_yoctonear(1)))
        .await
        .expect("second attempt is accepted");
        assert_eq!(transport.sent_nonces(), vec![6, 31]);
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

    fn bootstrap_signer() -> (
        SigningNear,
        Arc<UninitializedUniversalTransport>,
        Vec<Action>,
    ) {
        let key = SecretKey::generate_ed25519();
        let state_init =
            crate::protocol::UniversalStateInitV1::new().with_access_key(key.public_key());
        let account_id =
            crate::protocol::UniversalStateInit::from(state_init.clone()).derive_account_id();
        let transport = Arc::new(UninitializedUniversalTransport::default());
        let near = crate::Near::custom("http://bootstrap-again.invalid", "bootstrap-again")
            .transport(transport.clone())
            .build()
            .with_signer(InMemorySigner::new(account_id, key));
        let actions = vec![Action::universal_state_init(state_init, NearToken::ZERO)];
        (near, transport, actions)
    }

    #[tokio::test]
    async fn dropped_bootstrap_signature_does_not_shift_the_bootstrap_nonce() {
        // A self-signed bootstrap is checked in strict mode: only
        // bootstrap_nonce + 1 is accepted, however many were handed out.
        let (near, transport, actions) = bootstrap_signer();
        let account_id = near.account_id().clone();

        let dropped = near.sign(&account_id, actions.clone()).await.unwrap();
        assert_eq!(dropped.transaction.nonce, 100_000_001);

        let mut tx = near.wait_until::<Submitted>().transaction(&account_id);
        for action in actions {
            tx = tx.add_action(action);
        }
        tx.await.expect("bootstrap is sent");
        let sent = transport.sent.lock().unwrap();
        let tx = SignedTransaction::from_base64(&sent[0])
            .unwrap()
            .transaction;
        assert_eq!(tx.nonce, 100_000_001);
    }

    #[tokio::test]
    async fn bootstrap_retry_keeps_the_bootstrap_nonce() {
        // An InvalidNonce reporting the bootstrap nonce itself (another
        // bootstrap pending, say) must not push the retry to floor + 2.
        let (near, transport, actions) = bootstrap_signer();
        transport
            .send_tx_responses
            .lock()
            .unwrap()
            .push_back(invalid_nonce_body(100_000_001, 100_000_000));
        let account_id = near.account_id().clone();

        let mut tx = near.wait_until::<Submitted>().transaction(&account_id);
        for action in actions {
            tx = tx.add_action(action);
        }
        tx.await.expect("retry is sent");
        let sent = transport.sent.lock().unwrap();
        let nonces: Vec<u64> = sent
            .iter()
            .map(|tx| {
                SignedTransaction::from_base64(tx)
                    .unwrap()
                    .transaction
                    .nonce
            })
            .collect();
        assert_eq!(nonces, vec![100_000_001, 100_000_001]);
    }

    fn delegate_options() -> DelegateOptions {
        DelegateOptions {
            max_block_height: Some(1000),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn sign_delegate_draws_consecutive_nonces() {
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via("http://delegates.invalid", "delegates", transport, signer);

        let first = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        let second = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        assert_eq!(first.delegate_action.nonce, 6);
        assert_eq!(second.delegate_action.nonce, 7);

        // An explicit nonce still wins.
        let explicit = near
            .sign_delegate(
                account!("bob.testnet"),
                transfer(),
                DelegateOptions {
                    nonce: Some(42),
                    ..delegate_options()
                },
            )
            .await
            .unwrap();
        assert_eq!(explicit.delegate_action.nonce, 42);
    }

    #[tokio::test]
    async fn sign_delegate_reads_the_key_optimistically() {
        // A signed delegate action can't be retried with another nonce, so
        // its view must not trail nonces already used, as a final one can.
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via(
            "http://optimistic-delegate.invalid",
            "optimistic-delegate",
            transport.clone(),
            signer,
        );
        near.sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        assert_eq!(
            *transport.view_finalities.lock().unwrap(),
            vec![serde_json::json!("optimistic")]
        );
    }

    /// A send's final view can trail the optimistic view a delegate action
    /// drew its nonce from. Here the key was re-added at height 102, so the
    /// optimistic view (height 103) shows nonce 102_000_000 while the final
    /// one (height 100) still shows the old key's 50. The send's older view
    /// must not erase the delegate's reservation, or the next delegate action
    /// gets the same nonce and only one of the two can run.
    #[tokio::test]
    async fn an_older_view_keeps_a_delegate_reservation() {
        let transport = ScriptedNonceTransport::new(
            vec![(102_000_000, 103), (50, 100), (102_000_000, 103)],
            vec![],
        );
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via(
            "http://older-view.invalid",
            "older-view",
            transport.clone(),
            signer,
        );

        let first = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        let pending = near
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        let second = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        assert_eq!(
            *transport.view_finalities.lock().unwrap(),
            vec![
                serde_json::json!("optimistic"),
                serde_json::json!("final"),
                serde_json::json!("optimistic"),
            ]
        );
        assert_eq!(first.delegate_action.nonce, 102_000_001);
        assert_eq!(pending.transaction.nonce, 102_000_002);
        assert_eq!(second.delegate_action.nonce, 102_000_003);
    }

    #[tokio::test]
    async fn sign_delegate_after_pending_sign_takes_the_next_nonce() {
        let transport = ScriptedNonceTransport::new(vec![(5, 100)], vec![]);
        let signer = InMemorySigner::new(account!("alice.testnet"), SecretKey::generate_ed25519());
        let near = signing_via(
            "http://sign-then-delegate.invalid",
            "sign-then-delegate",
            transport,
            signer,
        );

        let pending = near
            .sign(account!("bob.testnet"), transfer())
            .await
            .unwrap();
        let delegate = near
            .sign_delegate(account!("bob.testnet"), transfer(), delegate_options())
            .await
            .unwrap();
        assert_eq!(pending.transaction.nonce, 6);
        assert_eq!(delegate.delegate_action.nonce, 7);
    }
}
