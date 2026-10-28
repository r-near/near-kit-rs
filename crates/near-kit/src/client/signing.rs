//! The signing client.

use std::fmt;
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::{Error, RpcError};
use crate::platform::MaybeSend;
use crate::types::{
    AccountId, Action, BlockReference, Final, SignedDelegateAction, SignedTransaction, Signer,
    ViewFunctionResult, WaitLevel,
};

use super::near::Near;
use super::transaction::{DelegateOptions, PreparedTransaction, Tx};

/// A [`Near`] client with a signer: sends transactions as the signer's
/// account.
///
/// Get one from [`Near::with_signer`] or [`SigningNear::from_env`]. It holds
/// the `Near` (shared RPC connection) and the signer behind `Arc`s, so
/// cloning is cheap; nonces live in a process-wide cache, so clones and
/// separately created clients for the same key don't collide.
///
/// `W` is the [`WaitLevel`] every transaction from this client waits for,
/// [`Final`] by default: the same finality reads use by default, so a read
/// after a write sees it. Pick another once with
/// [`wait_until`](Self::wait_until); the response type follows the level.
///
/// Reads go through [`near()`](Self::near): there is no `Deref` and no
/// forwarded read methods. `SigningNear` implements [`Viewer`](crate::Viewer)
/// and [`TxSender`](crate::TxSender), so generated contract clients and other
/// generic code accept it directly.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
/// use near_kit::protocol::Action;
/// use near_kit::signer::InMemorySigner;
/// use near_kit::transaction::Included;
///
/// # async fn example() -> Result<(), Error> {
/// let near = Near::testnet().build();
/// let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?));
///
/// alice
///     .transaction(account!("bob.testnet"))
///     .add_action(Action::transfer(NearToken::from_near(1)))
///     .await?.result()?;
///
/// // The same account, returning once each transaction is in a block.
/// let alice_fast = alice.wait_until::<Included>();
///
/// // Reads go through the read-only client.
/// let balance = alice.near().balance(alice.account_id()).await?;
/// # Ok(())
/// # }
/// ```
pub struct SigningNear<W: WaitLevel = Final> {
    near: Near,
    signer: Arc<dyn Signer>,
    _wait: PhantomData<fn() -> W>,
}

impl<W: WaitLevel> Clone for SigningNear<W> {
    fn clone(&self) -> Self {
        Self {
            near: self.near.clone(),
            signer: self.signer.clone(),
            _wait: PhantomData,
        }
    }
}

impl<W: WaitLevel> fmt::Debug for SigningNear<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningNear")
            .field("account_id", self.signer.account_id())
            .field("wait_until", &W::STATUS)
            .field("near", &self.near)
            .finish()
    }
}

impl SigningNear {
    /// Create a signing client from environment variables.
    ///
    /// Reads:
    /// - `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` (required): the account and
    ///   its private key (e.g. `"ed25519:..."`).
    /// - `NEAR_NETWORK` (optional): `"mainnet"`, `"testnet"`, or a custom RPC
    ///   URL. Defaults to `"testnet"`.
    /// - `NEAR_CHAIN_ID`: the chain ID. Required when `NEAR_NETWORK` is a
    ///   URL; optional for `"mainnet"` and `"testnet"`, where it overrides the
    ///   preset's chain ID.
    /// - `NEAR_MAX_NONCE_RETRIES` (optional): number of retries on
    ///   `InvalidNonce` errors. `0` means no retries. Defaults to `3`.
    ///
    /// The signer comes from [`signer::from_env`](crate::signer::from_env).
    /// For a read-only client, build a [`Near`] instead.
    ///
    /// # Example
    ///
    /// ```bash
    /// export NEAR_NETWORK=https://rpc.pinet.near.org
    /// export NEAR_CHAIN_ID=pinet
    /// export NEAR_ACCOUNT_ID=alice.testnet
    /// export NEAR_PRIVATE_KEY=ed25519:...
    /// export NEAR_MAX_NONCE_RETRIES=10
    /// ```
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let alice = SigningNear::from_env()?;
    /// alice
    ///     .transaction(account!("bob.testnet"))
    ///     .add_action(Action::transfer(NearToken::from_near(1)))
    ///     .await?.result()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `NEAR_ACCOUNT_ID` or `NEAR_PRIVATE_KEY` is missing
    /// - `NEAR_ACCOUNT_ID` or `NEAR_PRIVATE_KEY` is invalid
    /// - `NEAR_NETWORK` is a URL and `NEAR_CHAIN_ID` is not set
    ///   ([`Error::Config`])
    /// - `NEAR_MAX_NONCE_RETRIES` is set but not a valid integer
    pub fn from_env() -> Result<SigningNear, Error> {
        let network = std::env::var("NEAR_NETWORK").ok();
        let mut chain_id_override = std::env::var("NEAR_CHAIN_ID").ok();
        let signer = crate::signer::from_env()?;

        let mut builder = match network.as_deref() {
            Some("mainnet") => Near::mainnet(),
            Some("testnet") | None => Near::testnet(),
            Some(url) => {
                let chain_id = chain_id_override.take().ok_or_else(|| {
                    Error::Config(format!(
                        "NEAR_NETWORK is the URL {url}, so NEAR_CHAIN_ID must be set"
                    ))
                })?;
                Near::custom(url, chain_id)
            }
        };

        // NEAR_CHAIN_ID also overrides the mainnet/testnet presets.
        if let Some(id) = chain_id_override {
            builder = builder.chain_id(id);
        }

        if let Ok(retries) = std::env::var("NEAR_MAX_NONCE_RETRIES") {
            let retries: u32 = retries.parse().map_err(|_| {
                Error::Config(format!(
                    "NEAR_MAX_NONCE_RETRIES must be a non-negative integer, got: {retries}"
                ))
            })?;
            builder = builder.max_nonce_retries(retries);
        }

        Ok(builder.build().with_signer(signer))
    }
}

impl<W: WaitLevel> SigningNear<W> {
    pub(crate) fn new(near: Near, signer: Arc<dyn Signer>) -> Self {
        Self {
            near,
            signer,
            _wait: PhantomData,
        }
    }

    /// The read-only client this one signs on top of.
    pub fn near(&self) -> &Near {
        &self.near
    }

    /// The account transactions are signed and sent as.
    pub fn account_id(&self) -> &AccountId {
        self.signer.account_id()
    }

    /// The signer.
    ///
    /// Use `signer().public_key()` for the key the next transaction will
    /// use (it does not advance a
    /// [`RotatingSigner`](crate::signer::RotatingSigner)), or clone it to
    /// attach the same signer to another [`Near`].
    pub fn signer(&self) -> &Arc<dyn Signer> {
        &self.signer
    }

    /// The same account, waiting for `W2` instead of `W`.
    ///
    /// Transactions sent through the result resolve to `W2::Response`:
    /// [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome) for the
    /// executed levels, [`SendTxResponse`](crate::rpc::SendTxResponse) for
    /// [`Submitted`](crate::transaction::Submitted),
    /// [`Included`](crate::transaction::Included) and
    /// [`IncludedFinal`](crate::transaction::IncludedFinal).
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # use near_kit::transaction::Included;
    /// # async fn example(alice: SigningNear) -> Result<(), Error> {
    /// let response = alice
    ///     .wait_until::<Included>()
    ///     .transaction(account!("bob.testnet"))
    ///     .add_action(Action::transfer(NearToken::from_near(1)))
    ///     .await?;
    /// println!("tx hash: {}", response.transaction_hash);
    /// # Ok(())
    /// # }
    /// ```
    pub fn wait_until<W2: WaitLevel>(&self) -> SigningNear<W2> {
        SigningNear::new(self.near.clone(), self.signer.clone())
    }

    /// Set how many times a transaction is re-signed and re-sent after an
    /// `InvalidNonce`, `NonceTooLarge` or `Expired` error. `0` sends once.
    ///
    /// Like the wait level, this is sender policy: it applies to every
    /// transaction this client sends, and doesn't change the [`Near`] it was
    /// created from. The default is that `Near`'s setting (`3` unless
    /// configured).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::transaction::Included;
    /// # fn example(alice: SigningNear) {
    /// // A relayer that keeps retrying and returns once included.
    /// let relayer = alice.clone().max_nonce_retries(u32::MAX).wait_until::<Included>();
    /// # }
    /// ```
    pub fn max_nonce_retries(mut self, retries: u32) -> Self {
        self.near.max_nonce_retries = retries;
        self
    }

    /// Start a transaction to `receiver_id`.
    ///
    /// Add actions with [`Tx::add_action`] and `.await` the result. See
    /// [`Tx`] for an example.
    pub fn transaction(&self, receiver_id: impl Into<AccountId>) -> Tx<W> {
        Tx::new(self.clone(), receiver_id.into())
    }

    /// Create a typed contract client that sends as this account.
    ///
    /// `T` is the marker type `#[near_kit::contract]` generates; the result
    /// is its `<Name>Client<SigningNear<W>>`, whose `#[call]` methods resolve
    /// to `W::Response`. See [`Near::contract`] for a read-only client.
    pub fn contract<T: crate::Contract>(
        &self,
        contract_id: impl Into<AccountId>,
    ) -> T::Client<Self> {
        T::client(self.clone(), contract_id.into())
    }

    /// Sign a transaction without sending it.
    ///
    /// Resolves the nonce and block hash from the network and signs. Send the
    /// result later with [`Near::send_signed`], or hand it to someone else to
    /// submit.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # async fn example(alice: SigningNear) -> Result<(), Error> {
    /// let signed = alice
    ///     .sign(account!("bob.testnet"), vec![Action::transfer(NearToken::from_near(1))])
    ///     .await?;
    /// println!("Hash: {}", signed.get_hash());
    ///
    /// alice.near().send_signed(&signed).await?.result()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// To sign with a nonce and block hash you already have, without the
    /// network, build the [`Transaction`](crate::protocol::Transaction)
    /// yourself and sign its hash with the signer's key. This works for any
    /// [`Signer`](crate::signer::Signer), including keys you can't export:
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::{Action, Transaction};
    /// # use near_kit::signer::Signer;
    /// # async fn example(alice: SigningNear, nonce: u64, block_hash: CryptoHash) -> Result<(), Error> {
    /// let signer = alice.signer();
    /// // Claim the key once: a rotating signer hands out a different key per call.
    /// let key = signer.key();
    /// let tx = Transaction::new(
    ///     signer.account_id().clone(),
    ///     key.public_key().clone(),
    ///     nonce,
    ///     account!("bob.testnet"),
    ///     block_hash,
    ///     vec![Action::transfer(NearToken::from_near(1))],
    /// );
    /// let signature = key.sign(tx.get_hash().as_bytes()).await?;
    /// let signed = tx.into_signed(signature);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Validation`] if `actions` is empty or a state init
    /// targets another receiver, otherwise RPC and signing errors.
    pub async fn sign(
        &self,
        receiver_id: impl Into<AccountId>,
        actions: Vec<Action>,
    ) -> Result<SignedTransaction, Error> {
        PreparedTransaction::new(
            &self.near,
            self.signer.clone(),
            receiver_id.into(),
            actions,
            "Transaction must have at least one action",
        )?
        .sign()
        .await
    }

    /// Build and sign a delegate action for a meta-transaction (NEP-366).
    ///
    /// The user signs `actions` off-chain; a relayer wraps the result in a
    /// transaction of its own (with
    /// [`Action::delegate`](crate::protocol::Action::delegate), sent to
    /// [`sender_id()`](SignedDelegateAction::sender_id)) and pays the gas.
    /// [`to_base64()`](SignedDelegateAction::to_base64) is the encoding to hand
    /// a relayer over HTTP.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # use near_kit::rpc::OutcomeError;
    /// # use near_kit::transaction::FunctionCall;
    /// # async fn example(alice: SigningNear, relayer: SigningNear) -> Result<(), Error> {
    /// // User signs the delegate action
    /// let call = FunctionCall::new("add_message")
    ///     .args(serde_json::json!({ "text": "Hello!" }))
    ///     .gas(Gas::from_tgas(30));
    /// let signed = alice
    ///     .sign_delegate(account!("guestbook.testnet"), vec![call.try_into()?], Default::default())
    ///     .await?;
    ///
    /// // Relayer submits it and pays the gas
    /// let outcome = relayer
    ///     .transaction(signed.sender_id())
    ///     .add_action(Action::delegate(signed))
    ///     .await?;
    /// // `result()` doesn't see the inner call; `receipt_failures()` does.
    /// outcome.result()?;
    /// if let Some(error) = outcome.receipt_failures().next() {
    ///     return Err(OutcomeError::ActionFailed(Box::new(error.clone())).into());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Validation`] if `actions` is empty or contains a
    /// delegate action, otherwise RPC and signing errors.
    pub async fn sign_delegate(
        &self,
        receiver_id: impl Into<AccountId>,
        actions: Vec<Action>,
        options: DelegateOptions,
    ) -> Result<SignedDelegateAction, Error> {
        PreparedTransaction::new(
            &self.near,
            self.signer.clone(),
            receiver_id.into(),
            actions,
            "Delegate action requires at least one action",
        )?
        .delegate(options)
        .await
    }
}

impl<W: WaitLevel> crate::Viewer for SigningNear<W> {
    type ViewError = RpcError;

    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, RpcError>> + MaybeSend {
        crate::Viewer::view_function(&self.near, contract_id, method, args, block)
    }
}

impl<W: WaitLevel> crate::TxSender for SigningNear<W> {
    type Output = W::Response;
    type SendError = Error;

    fn account_id(&self) -> &AccountId {
        self.signer.account_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<W::Response, Error>> + MaybeSend {
        let mut tx = self.transaction(receiver_id);
        tx.actions = actions;
        tx.into_future()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account;
    use crate::signer::InMemorySigner;
    use crate::transaction::{ExecutedOptimistic, Included};

    const ALICE_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

    fn alice() -> SigningNear {
        Near::testnet().build().with_signer(InMemorySigner::new(
            account!("alice.testnet"),
            ALICE_KEY.parse().unwrap(),
        ))
    }

    #[test]
    fn identity_accessors() {
        let alice = alice();
        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(
            alice.signer().public_key(),
            InMemorySigner::new(account!("alice.testnet"), ALICE_KEY.parse().unwrap())
                .public_key()
                .clone()
        );
        assert!(alice.near().chain_id().is_testnet());
    }

    #[test]
    fn wait_until_keeps_the_account_and_connection() {
        let alice = alice();
        let alice_fast: SigningNear<ExecutedOptimistic> = alice.wait_until::<ExecutedOptimistic>();
        let alice_included: SigningNear<Included> = alice_fast.wait_until();
        assert_eq!(alice_included.account_id(), alice.account_id());
        assert!(Arc::ptr_eq(alice_included.signer(), alice.signer()));
        assert_eq!(alice_included.near().rpc().url(), alice.near().rpc().url());
    }

    #[test]
    fn max_nonce_retries_is_per_sender() {
        let alice = alice();
        let relayer = alice.clone().max_nonce_retries(u32::MAX);
        assert_eq!(relayer.near().max_nonce_retries, u32::MAX);
        assert_eq!(alice.near().max_nonce_retries, 3);
        let included = relayer.wait_until::<Included>();
        assert_eq!(included.near().max_nonce_retries, u32::MAX);
    }

    #[test]
    fn debug_shows_account_and_wait_level() {
        let debug = format!("{:?}", alice());
        assert!(debug.contains("alice.testnet"), "{debug}");
        assert!(debug.contains("Final"), "{debug}");
        let debug = format!("{:?}", alice().wait_until::<Included>());
        assert!(debug.contains("Included"), "{debug}");
    }

    #[test]
    fn token_clients_take_account_ids_without_network() {
        use crate::tokens::{FungibleToken, NonFungibleToken};
        use crate::types::AccountIdRef;

        const WRAP: &AccountIdRef = AccountIdRef::new_or_panic("wrap.testnet");

        let alice = alice();
        let ft = alice.contract::<FungibleToken>(WRAP);
        assert_eq!(ft.contract_id().as_str(), "wrap.testnet");

        let nft_id: AccountId = "nft.testnet".parse().unwrap();
        assert_eq!(
            alice
                .contract::<NonFungibleToken>(nft_id.clone())
                .contract_id(),
            &nft_id
        );
        assert_eq!(
            alice.contract::<NonFungibleToken>(&nft_id).contract_id(),
            &nft_id
        );
    }

    // Environment variable tests are consolidated into a single test because
    // they modify global state and would race with each other if run in
    // parallel. Each scenario is tested sequentially within this test.
    #[test]
    fn test_from_env_scenarios() {
        fn clear_env() {
            // SAFETY: This is a test and we control the execution
            unsafe {
                std::env::remove_var("NEAR_CHAIN_ID");
                std::env::remove_var("NEAR_NETWORK");
                std::env::remove_var("NEAR_ACCOUNT_ID");
                std::env::remove_var("NEAR_PRIVATE_KEY");
                std::env::remove_var("NEAR_MAX_NONCE_RETRIES");
            }
        }
        fn set_credentials() {
            // SAFETY: as above
            unsafe {
                std::env::set_var("NEAR_ACCOUNT_ID", "alice.testnet");
                std::env::set_var("NEAR_PRIVATE_KEY", ALICE_KEY);
            }
        }
        fn expect_error_mentioning(needle: &str) {
            let err = SigningNear::from_env().expect_err("expected an error");
            assert!(err.to_string().contains(needle), "{needle}: {err}");
        }

        // No vars: a signing client needs credentials.
        clear_env();
        expect_error_mentioning("NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY");

        // Credentials only: testnet with default retries.
        clear_env();
        set_credentials();
        {
            let near = SigningNear::from_env().unwrap();
            assert_eq!(near.account_id().as_str(), "alice.testnet");
            assert!(near.near().chain_id().is_testnet());
            assert!(
                near.near().rpc().url().contains("test")
                    || near.near().rpc().url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.near().rpc().url()
            );
            assert_eq!(near.near().max_nonce_retries, 3);
        }

        // Mainnet network
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "mainnet");
        }
        {
            let near = SigningNear::from_env().unwrap();
            assert!(near.near().chain_id().is_mainnet());
        }

        // Custom URL without NEAR_CHAIN_ID: a config error, no "custom" default
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "https://custom-rpc.example.com");
        }
        {
            let err = SigningNear::from_env().expect_err("expected an error");
            assert!(
                matches!(&err, Error::Config(msg) if msg.contains("NEAR_CHAIN_ID")),
                "{err:?}"
            );
        }

        // Account without key
        clear_env();
        unsafe {
            std::env::set_var("NEAR_ACCOUNT_ID", "alice.testnet");
        }
        expect_error_mentioning("NEAR_PRIVATE_KEY");

        // Key without account
        clear_env();
        unsafe {
            std::env::set_var("NEAR_PRIVATE_KEY", ALICE_KEY);
        }
        expect_error_mentioning("NEAR_ACCOUNT_ID");

        // Invalid key
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_PRIVATE_KEY", "not-a-key");
        }
        assert!(SigningNear::from_env().is_err());

        // Custom max_nonce_retries, including zero
        for (value, expected) in [("10", 10), ("0", 0)] {
            clear_env();
            set_credentials();
            unsafe {
                std::env::set_var("NEAR_MAX_NONCE_RETRIES", value);
            }
            let near = SigningNear::from_env().unwrap();
            assert_eq!(near.near().max_nonce_retries, expected);
        }

        // Invalid max_nonce_retries
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_MAX_NONCE_RETRIES", "abc");
        }
        expect_error_mentioning("NEAR_MAX_NONCE_RETRIES");

        // NEAR_CHAIN_ID overrides chain_id for a custom network
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "https://rpc.pinet.near.org");
            std::env::set_var("NEAR_CHAIN_ID", "pinet");
        }
        {
            let near = SigningNear::from_env().unwrap();
            assert_eq!(near.near().rpc().url(), "https://rpc.pinet.near.org");
            assert_eq!(near.near().chain_id().as_str(), "pinet");
        }

        // NEAR_CHAIN_ID without NEAR_NETWORK keeps the testnet RPC
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_CHAIN_ID", "my-chain");
        }
        {
            let near = SigningNear::from_env().unwrap();
            assert!(
                near.near().rpc().url().contains("test")
                    || near.near().rpc().url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.near().rpc().url()
            );
            assert_eq!(near.near().chain_id().as_str(), "my-chain");
        }

        clear_env();
    }
}
