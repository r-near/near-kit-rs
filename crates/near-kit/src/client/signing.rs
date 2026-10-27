//! The signing client.

use std::fmt;
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::{Error, RpcError};
use crate::platform::MaybeSend;
use crate::types::nep413::{SignMessageParams, SignedMessage};
use crate::types::{
    AccountId, Action, BlockReference, ExecutedOptimistic, InMemorySigner, SignedTransaction,
    Signer, ViewFunctionResult, WaitLevel,
};

use super::near::Near;
use super::transaction::{DelegateOptions, DelegateResult, PreparedTransaction, Tx};

/// A [`Near`] client with a signer: sends transactions as the signer's
/// account.
///
/// Get one from [`Near::with_signer`] or [`SigningNear::from_env`]. It holds
/// the `Near` (shared RPC connection) and the signer behind `Arc`s, so
/// cloning is cheap; nonces live in a process-wide cache, so clones and
/// separately created clients for the same key don't collide.
///
/// `W` is the [`WaitLevel`] every transaction from this client waits for,
/// [`ExecutedOptimistic`] by default. Pick another once with
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
/// use near_kit::transaction::Final;
///
/// # async fn example() -> Result<(), Error> {
/// let near = Near::testnet().build();
/// let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
///
/// alice
///     .transaction(account!("bob.testnet"))
///     .add_action(Action::transfer(NearToken::from_near(1)))
///     .await?;
///
/// // The same account, waiting for finality on every transaction.
/// let alice_final = alice.wait_until::<Final>();
///
/// // Reads go through the read-only client.
/// let balance = alice.near().balance(alice.account_id()).await?;
/// # Ok(())
/// # }
/// ```
pub struct SigningNear<W: WaitLevel = ExecutedOptimistic> {
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
    /// - `NEAR_CHAIN_ID` (optional): overrides the chain ID inferred from
    ///   `NEAR_NETWORK`, including the `"mainnet"` and `"testnet"` presets.
    ///   Typically only needed for custom networks.
    /// - `NEAR_MAX_NONCE_RETRIES` (optional): number of retries on
    ///   `InvalidNonce` errors. `0` means no retries. Defaults to `3`.
    ///
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
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `NEAR_ACCOUNT_ID` or `NEAR_PRIVATE_KEY` is missing
    /// - `NEAR_ACCOUNT_ID` or `NEAR_PRIVATE_KEY` is invalid
    /// - `NEAR_MAX_NONCE_RETRIES` is set but not a valid integer
    pub fn from_env() -> Result<SigningNear, Error> {
        let network = std::env::var("NEAR_NETWORK").ok();
        let mut chain_id_override = std::env::var("NEAR_CHAIN_ID").ok();
        let account_id = std::env::var("NEAR_ACCOUNT_ID").ok();
        let private_key = std::env::var("NEAR_PRIVATE_KEY").ok();

        let signer = match (account_id, private_key) {
            (Some(account), Some(key)) => InMemorySigner::new(account, key)?,
            (Some(_), None) => {
                return Err(Error::Config(
                    "NEAR_ACCOUNT_ID is set but NEAR_PRIVATE_KEY is missing".into(),
                ));
            }
            (None, Some(_)) => {
                return Err(Error::Config(
                    "NEAR_PRIVATE_KEY is set but NEAR_ACCOUNT_ID is missing".into(),
                ));
            }
            (None, None) => {
                return Err(Error::Config(
                    "NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY must be set".into(),
                ));
            }
        };

        let mut builder = match network.as_deref() {
            Some("mainnet") => Near::mainnet(),
            Some("testnet") | None => Near::testnet(),
            Some(url) => {
                let chain_id = chain_id_override
                    .take()
                    .unwrap_or_else(|| "custom".to_string());
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
    /// let outcome = alice.near().send_signed(&signed).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::InvalidTransaction`] if `actions` is empty, otherwise RPC
    /// and signing errors.
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
    /// [`sender_id()`](DelegateResult::sender_id)) and pays the gas.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::protocol::Action;
    /// # use near_kit::transaction::FunctionCall;
    /// # async fn example(alice: SigningNear, relayer: SigningNear) -> Result<(), Error> {
    /// // User signs the delegate action
    /// let call = FunctionCall::new("add_message")
    ///     .args(serde_json::json!({ "text": "Hello!" }))
    ///     .gas(Gas::from_tgas(30));
    /// let result = alice
    ///     .delegate(account!("guestbook.testnet"), vec![call.try_into()?], Default::default())
    ///     .await?;
    ///
    /// // Relayer submits it and pays the gas
    /// relayer
    ///     .transaction(result.sender_id())
    ///     .add_action(Action::delegate(result.signed_delegate_action))
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::InvalidTransaction`] if `actions` is empty or contains a
    /// delegate action, otherwise RPC and signing errors.
    pub async fn delegate(
        &self,
        receiver_id: impl Into<AccountId>,
        actions: Vec<Action>,
        options: DelegateOptions,
    ) -> Result<DelegateResult, Error> {
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

    /// Sign a message for off-chain authentication (NEP-413).
    ///
    /// Proves ownership of the account without a transaction or gas, e.g. for
    /// a web login.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::nep413;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let signed = alice.sign_message(nep413::SignMessageParams {
    ///     message: "Login to MyApp".to_string(),
    ///     recipient: "myapp.com".to_string(),
    ///     nonce: nep413::generate_nonce(),
    ///     callback_url: None,
    ///     state: None,
    /// }).await?;
    ///
    /// println!("Signed by: {}", signed.account_id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// See <https://github.com/near/NEPs/blob/master/neps/nep-0413.md>.
    pub async fn sign_message(&self, params: SignMessageParams) -> Result<SignedMessage, Error> {
        self.signer
            .key()
            .sign_nep413(self.signer.account_id(), &params)
            .await
            .map_err(Error::Signing)
    }

    /// Get a fungible token client for a NEP-141 contract.
    ///
    /// The client reads through this client's [`Near`] and sends its calls
    /// as this account, waiting for `W`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = alice.ft(account!("wrap.near"));
    ///
    /// let meta = token.metadata().await?;
    /// println!("{} ({})", meta.name, meta.symbol);
    ///
    /// let balance = token.balance_of("alice.near").await?;
    /// println!("Balance: {}", balance);
    /// # Ok(())
    /// # }
    /// ```
    pub fn ft(&self, contract: impl Into<AccountId>) -> crate::tokens::FungibleToken<W> {
        crate::tokens::FungibleToken::new(self.clone(), contract.into())
    }

    /// Get a non-fungible token client for a NEP-171 contract.
    ///
    /// Like [`ft`](Self::ft), it reads through [`Near`] and sends as this
    /// account.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let nft = alice.nft(account!("nft-contract.near"));
    ///
    /// if let Some(token) = nft.token("token-123").await? {
    ///     println!("Owner: {}", token.owner_id);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn nft(&self, contract: impl Into<AccountId>) -> crate::tokens::NonFungibleToken<W> {
        crate::tokens::NonFungibleToken::new(self.clone(), contract.into())
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
    use crate::transaction::{Final, Included};

    const ALICE_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

    fn alice() -> SigningNear {
        Near::testnet()
            .build()
            .with_signer(InMemorySigner::new("alice.testnet", ALICE_KEY).unwrap())
    }

    #[test]
    fn identity_accessors() {
        let alice = alice();
        assert_eq!(alice.account_id().as_str(), "alice.testnet");
        assert_eq!(
            alice.signer().public_key(),
            InMemorySigner::new("alice.testnet", ALICE_KEY)
                .unwrap()
                .public_key()
                .clone()
        );
        assert!(alice.near().chain_id().is_testnet());
    }

    #[test]
    fn wait_until_keeps_the_account_and_connection() {
        let alice = alice();
        let alice_final: SigningNear<Final> = alice.wait_until::<Final>();
        let alice_included: SigningNear<Included> = alice_final.wait_until();
        assert_eq!(alice_included.account_id(), alice.account_id());
        assert!(Arc::ptr_eq(alice_included.signer(), alice.signer()));
        assert_eq!(alice_included.near().rpc_url(), alice.near().rpc_url());
    }

    #[test]
    fn debug_shows_account_and_wait_level() {
        let debug = format!("{:?}", alice().wait_until::<Final>());
        assert!(debug.contains("alice.testnet"), "{debug}");
        assert!(debug.contains("Final"), "{debug}");
    }

    #[test]
    fn token_helpers_take_account_ids_without_network() {
        use crate::types::AccountIdRef;

        const WRAP: &AccountIdRef = AccountIdRef::new_or_panic("wrap.testnet");

        let alice = alice();
        let ft = alice.ft(WRAP);
        assert_eq!(ft.contract_id().as_str(), "wrap.testnet");

        let nft_id: AccountId = "nft.testnet".parse().unwrap();
        assert_eq!(alice.nft(nft_id.clone()).contract_id(), &nft_id);
        assert_eq!(alice.nft(&nft_id).contract_id(), &nft_id);
    }

    #[tokio::test]
    async fn sign_message_uses_the_signer_account() {
        let signed = alice()
            .sign_message(SignMessageParams {
                message: "hello".to_string(),
                recipient: "app.example".to_string(),
                nonce: [7; 32],
                callback_url: None,
                state: None,
            })
            .await
            .unwrap();
        assert_eq!(signed.account_id.as_str(), "alice.testnet");
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
                near.near().rpc_url().contains("test")
                    || near.near().rpc_url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.near().rpc_url()
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

        // Custom URL
        clear_env();
        set_credentials();
        unsafe {
            std::env::set_var("NEAR_NETWORK", "https://custom-rpc.example.com");
        }
        {
            let near = SigningNear::from_env().unwrap();
            assert_eq!(near.near().rpc_url(), "https://custom-rpc.example.com");
            assert_eq!(near.near().chain_id().as_str(), "custom");
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
            assert_eq!(near.near().rpc_url(), "https://rpc.pinet.near.org");
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
                near.near().rpc_url().contains("test")
                    || near.near().rpc_url().contains("fastnear"),
                "Expected testnet URL, got: {}",
                near.near().rpc_url()
            );
            assert_eq!(near.near().chain_id().as_str(), "my-chain");
        }

        clear_env();
    }
}
