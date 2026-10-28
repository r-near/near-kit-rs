//! Fungible token client (NEP-141).

use std::sync::Arc;

use crate::trace::{self, Instrument};
// The constructor `#[contract]` clients build their calls with.
use near_kit_primitives::__private::call as call_through;
use serde_json::json;
use tokio::sync::OnceCell;

use crate::Near;
use crate::transaction::FunctionCall;
use crate::types::{AccountId, Gas, NearToken};
use crate::{Call, ContractError};
use crate::{Contract, TxSender, Viewer};

use super::types::{FtMetadata, StorageBalance, StorageBalanceBounds};
use super::{Stringified, read};

// =============================================================================
// FungibleToken
// =============================================================================

/// A NEP-141 fungible token contract.
///
/// The marker for [`Near::contract`](crate::Near::contract) and
/// [`SigningNear::contract`](crate::SigningNear::contract), like the unit
/// struct `#[contract]` generates. Its client is [`FungibleTokenClient`]; for
/// another backend, use [`Contract::client`].
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
/// use near_kit::standards::FungibleToken;
///
/// # async fn example(near: Near) -> Result<(), near_kit::Error> {
/// let token = near.contract::<FungibleToken>(account!("wrap.near"));
///
/// // Get metadata
/// let meta = token.metadata().await?;
/// println!("{} has {} decimals", meta.symbol, meta.decimals);
///
/// // Get a balance, in the token's smallest unit
/// let balance: u128 = token.balance_of(account!("alice.near")).await?;
/// println!("Balance: {balance}");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct FungibleToken;

/// Client for a NEP-141 fungible token contract, over a backend `C`.
///
/// Views need `C: Viewer` and read at optimistic finality; calls need
/// `C: TxSender + Clone` and return a [`Call`] that sends one transaction
/// when awaited. `near.contract::<FungibleToken>(id)` gives a read-only
/// client and `alice.contract::<FungibleToken>(id)` one that can also
/// send.
///
/// # Caching
///
/// Token metadata and storage bounds are fetched on first use and cached.
/// Clones share the cache.
pub struct FungibleTokenClient<C = Near> {
    backend: C,
    contract_id: AccountId,
    metadata: Arc<OnceCell<FtMetadata>>,
    storage_bounds: Arc<OnceCell<StorageBalanceBounds>>,
}

impl Contract for FungibleToken {
    type Client<C> = FungibleTokenClient<C>;

    fn client<C>(backend: C, contract_id: AccountId) -> FungibleTokenClient<C> {
        FungibleTokenClient {
            backend,
            contract_id,
            metadata: Arc::new(OnceCell::new()),
            storage_bounds: Arc::new(OnceCell::new()),
        }
    }
}

impl<C> FungibleTokenClient<C> {
    /// Get the contract ID.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }
}

// =============================================================================
// View Methods
// =============================================================================

impl<C: Viewer> FungibleTokenClient<C> {
    /// Get token metadata (ft_metadata).
    ///
    /// Metadata is cached after the first call.
    pub async fn metadata(&self) -> Result<&FtMetadata, ContractError<C::ViewError>> {
        self.metadata
            .get_or_try_init(|| read(&self.backend, &self.contract_id, "ft_metadata", None))
            .await
    }

    /// Get token balance for an account (ft_balance_of).
    ///
    /// The balance is in the token's smallest unit. To display it, scale by
    /// the `decimals` from [`metadata`](Self::metadata).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::FungibleToken;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let token = near.contract::<FungibleToken>(account!("wrap.near"));
    ///
    /// let balance: u128 = token.balance_of(account!("alice.near")).await?;
    /// let meta = token.metadata().await?;
    /// let whole = balance / 10u128.pow(meta.decimals.into());
    /// println!("{whole} {} (and {balance} raw)", meta.symbol);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn balance_of(
        &self,
        account_id: impl Into<AccountId>,
    ) -> Result<u128, ContractError<C::ViewError>> {
        let account_id = account_id.into();
        let span = trace::debug_span!("ft_balance_of", contract = %self.contract_id, %account_id);

        async {
            let Stringified(raw) = read(
                &self.backend,
                &self.contract_id,
                "ft_balance_of",
                Some(json!({ "account_id": account_id })),
            )
            .await?;
            Ok(raw)
        }
        .instrument(span)
        .await
    }

    /// Get total token supply (ft_total_supply), in the token's smallest unit.
    pub async fn total_supply(&self) -> Result<u128, ContractError<C::ViewError>> {
        let Stringified(raw) =
            read(&self.backend, &self.contract_id, "ft_total_supply", None).await?;
        Ok(raw)
    }

    // =========================================================================
    // Storage Methods (NEP-145)
    // =========================================================================

    /// Check if an account is registered on this token contract.
    ///
    /// An account must be registered (via `storage_deposit`) before it can
    /// receive tokens.
    pub async fn is_registered(
        &self,
        account_id: impl Into<AccountId>,
    ) -> Result<bool, ContractError<C::ViewError>> {
        Ok(self.storage_balance_of(account_id).await?.is_some())
    }

    /// Get storage balance for an account (storage_balance_of).
    ///
    /// Returns `None` if the account is not registered.
    pub async fn storage_balance_of(
        &self,
        account_id: impl Into<AccountId>,
    ) -> Result<Option<StorageBalance>, ContractError<C::ViewError>> {
        let account_id = account_id.into();
        read(
            &self.backend,
            &self.contract_id,
            "storage_balance_of",
            Some(json!({ "account_id": account_id })),
        )
        .await
    }

    /// Get storage balance bounds for this token contract.
    ///
    /// Returns the minimum and maximum storage deposit amounts.
    /// The minimum is typically needed for [`storage_deposit`](Self::storage_deposit).
    /// Cached after the first call.
    pub async fn storage_balance_bounds(
        &self,
    ) -> Result<&StorageBalanceBounds, ContractError<C::ViewError>> {
        self.storage_bounds
            .get_or_try_init(|| {
                read(
                    &self.backend,
                    &self.contract_id,
                    "storage_balance_bounds",
                    None,
                )
            })
            .await
    }
}

// =============================================================================
// Call Methods
// =============================================================================

impl<C: TxSender + Clone> FungibleTokenClient<C> {
    fn call(&self, function_call: FunctionCall) -> Call<C> {
        call_through(
            self.backend.clone(),
            self.contract_id.clone(),
            function_call,
        )
    }

    /// Register an account on this token contract (storage_deposit).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::FungibleToken;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = alice.contract::<FungibleToken>(account!("wrap.near"));
    ///
    /// // Register bob with the contract's minimum deposit
    /// let bounds = token.storage_balance_bounds().await?;
    /// token.storage_deposit(account!("bob.near"), bounds.min).await?.result()?;
    ///
    /// // Or with a known amount
    /// token
    ///     .storage_deposit(account!("bob.near"), NearToken::from_millinear(50))
    ///     .await?.result()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn storage_deposit(&self, account_id: impl Into<AccountId>, deposit: NearToken) -> Call<C> {
        let account_id = account_id.into();
        self.call(
            FunctionCall::new("storage_deposit")
                .args(json!({ "account_id": account_id, "registration_only": true }))
                .deposit(deposit)
                .gas(Gas::from_tgas(30)),
        )
    }

    /// Transfer tokens to a receiver (ft_transfer), with an optional memo.
    ///
    /// Amount is in the token's smallest unit, as [`balance_of`](Self::balance_of)
    /// returns it.
    ///
    /// # Security
    ///
    /// This automatically attaches 1 yoctoNEAR as required by NEP-141 for
    /// security (prevents function-call access key abuse).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::FungibleToken;
    /// # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = alice.contract::<FungibleToken>(account!("wrap.near"));
    ///
    /// token.transfer(account!("bob.near"), 1_500_000_u128, None).await?.result()?;
    ///
    /// // Or send a whole balance, with a memo
    /// let balance = token.balance_of(account!("alice.near")).await?;
    /// token
    ///     .transfer(account!("bob.near"), balance, Some("rent".into()))
    ///     .await?.result()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(
        &self,
        receiver_id: impl Into<AccountId>,
        amount: impl Into<u128>,
        memo: Option<String>,
    ) -> Call<C> {
        let receiver_id = receiver_id.into();
        trace::debug!(contract = %self.contract_id, receiver = %receiver_id, "ft_transfer");

        let mut args = json!({
            "receiver_id": receiver_id,
            "amount": amount.into().to_string(),
        });
        if let Some(memo) = memo {
            args["memo"] = memo.into();
        }
        self.call(
            FunctionCall::new("ft_transfer")
                .args(args)
                .deposit(NearToken::from_yoctonear(1))
                .gas(Gas::from_tgas(30)),
        )
    }

    /// Transfer tokens with a callback to the receiver (ft_transfer_call).
    ///
    /// This calls `ft_on_transfer` on the receiver contract, allowing it to
    /// handle the tokens (e.g., for swaps, deposits, etc.).
    ///
    /// The receiver can return unused tokens, which will be refunded to the sender.
    ///
    /// `.result()?` doesn't tell you the receiver took the tokens: if
    /// `ft_on_transfer` fails, the token's `ft_resolve_transfer` callback
    /// handles the failure and tries to refund the sender, so the transaction
    /// still succeeds. The refund is capped at what the receiver still holds,
    /// so check the amount the call returns, which is what the receiver
    /// kept; [`receipt_failures`](crate::rpc::FinalExecutionOutcome::receipt_failures)
    /// shows why `ft_on_transfer` failed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::FungibleToken;
    /// # async fn example(alice: SigningNear) -> Result<(), Box<dyn std::error::Error>> {
    /// let token = alice.contract::<FungibleToken>(account!("wrap.near"));
    ///
    /// let outcome = token
    ///     .transfer_call(account!("defi.near"), 1_000_000_u128, r#"{"action":"deposit"}"#)
    ///     .await?;
    /// // The amount the receiver kept, as a JSON string.
    /// let used: u128 = outcome.json::<String>()?.parse()?;
    /// if used < 1_000_000 {
    ///     let reason = outcome.receipt_failures().next().map(|e| e.to_string());
    ///     return Err(format!("the receiver kept only {used} ({reason:?})").into());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl Into<AccountId>,
        amount: impl Into<u128>,
        msg: impl Into<String>,
    ) -> Call<C> {
        let receiver_id = receiver_id.into();
        trace::debug!(contract = %self.contract_id, receiver = %receiver_id, "ft_transfer_call");

        self.call(
            FunctionCall::new("ft_transfer_call")
                .args(json!({
                    "receiver_id": receiver_id,
                    "amount": amount.into().to_string(),
                    "msg": msg.into(),
                }))
                .deposit(NearToken::from_yoctonear(1))
                .gas(Gas::from_tgas(100)),
        )
    }
}

impl<C: Clone> Clone for FungibleTokenClient<C> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            contract_id: self.contract_id.clone(),
            metadata: self.metadata.clone(),
            storage_bounds: self.storage_bounds.clone(),
        }
    }
}

impl<C> std::fmt::Debug for FungibleTokenClient<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FungibleTokenClient")
            .field("contract_id", &self.contract_id)
            .field("metadata_cached", &self.metadata.initialized())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account;
    use crate::tokens::mock::{Mock, sent_call};
    use crate::transaction::CodecError;

    fn metadata() -> serde_json::Value {
        json!({
            "spec": "ft-1.0.0",
            "name": "Test Token",
            "symbol": "TEST",
            "decimals": 6,
        })
    }

    #[test]
    fn clones_share_metadata_and_storage_caches() {
        let token = FungibleToken::client(Mock::default(), account!("token.near"));
        token
            .metadata
            .set(FtMetadata {
                spec: "ft-1.0.0".to_string(),
                name: "Test Token".to_string(),
                symbol: "TEST".to_string(),
                decimals: 6,
                icon: None,
                reference: None,
                reference_hash: None,
            })
            .unwrap();
        token
            .storage_bounds
            .set(StorageBalanceBounds {
                min: NearToken::from_yoctonear(1),
                max: None,
            })
            .unwrap();

        let cloned = token.clone();

        assert!(Arc::ptr_eq(&token.metadata, &cloned.metadata));
        assert!(Arc::ptr_eq(&token.storage_bounds, &cloned.storage_bounds));
        assert_eq!(cloned.metadata.get().unwrap().symbol, "TEST");
        assert_eq!(
            cloned.storage_bounds.get().unwrap().min,
            NearToken::from_yoctonear(1)
        );
    }

    #[test]
    fn view_futures_are_send_for_sync_backends() {
        fn assert_send<T: Send>(_: T) {}
        let token = FungibleToken::client(Mock::default(), account!("token.near"));
        assert_send(token.metadata());
        assert_send(token.balance_of(account!("alice.near")));
        assert_send(token.storage_balance_bounds());
        let near_token = crate::Near::custom("http://127.0.0.1:1", "test")
            .build()
            .contract::<FungibleToken>(account!("token.near"));
        assert_send(near_token.balance_of(account!("alice.near")));
    }

    #[tokio::test]
    async fn views_decode_string_amounts_and_cache_metadata() {
        let mock = Mock::default()
            .answer("ft_metadata", metadata())
            .answer("ft_balance_of", json!("1500000"))
            .answer("ft_total_supply", json!("9000000"));
        let token = FungibleToken::client(mock.clone(), account!("token.near"));

        assert_eq!(
            token.balance_of(account!("alice.near")).await.unwrap(),
            1_500_000
        );
        assert_eq!(token.total_supply().await.unwrap(), 9_000_000);
        assert_eq!(token.metadata().await.unwrap().decimals, 6);
        assert_eq!(token.metadata().await.unwrap().symbol, "TEST");

        let seen = mock.seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            [
                (
                    "ft_balance_of".to_owned(),
                    json!({ "account_id": "alice.near" })
                ),
                ("ft_total_supply".to_owned(), serde_json::Value::Null),
                ("ft_metadata".to_owned(), serde_json::Value::Null),
            ],
            "amounts don't read metadata, metadata is read once, and \
             no-argument views send no arguments"
        );
    }

    #[tokio::test]
    async fn view_errors_keep_the_backend_error_and_bad_numbers_are_codec_errors() {
        let mock = Mock::default().answer("ft_balance_of", json!("not a number"));
        let token = FungibleToken::client(mock, account!("token.near"));

        assert!(matches!(
            token.balance_of(account!("alice.near")).await,
            Err(ContractError::Codec(CodecError::Json(_)))
        ));
        assert!(matches!(
            token.storage_balance_bounds().await,
            Err(ContractError::Backend(message)) if message.contains("storage_balance_bounds")
        ));
    }

    #[tokio::test]
    async fn calls_preserve_nep_141_arguments() {
        let token = FungibleToken::client(Mock::default(), account!("token.near"));
        let one_yocto = 1;
        let tgas = Gas::from_tgas(1).as_gas();

        assert_eq!(
            sent_call(
                token
                    .transfer(account!("alice.near"), 7_u128, None)
                    .await
                    .unwrap()
            ),
            (
                "ft_transfer".to_owned(),
                json!({"receiver_id": "alice.near", "amount": "7"}),
                one_yocto,
                30 * tgas
            )
        );
        assert_eq!(
            sent_call(
                token
                    .transfer(account!("alice.near"), 7_u128, Some("memo".into()))
                    .await
                    .unwrap()
            )
            .1,
            json!({"receiver_id": "alice.near", "amount": "7", "memo": "memo"})
        );
        assert_eq!(
            sent_call(
                token
                    .transfer_call(account!("defi.near"), 3_u128, "msg")
                    .await
                    .unwrap()
            ),
            (
                "ft_transfer_call".to_owned(),
                json!({"receiver_id": "defi.near", "amount": "3", "msg": "msg"}),
                one_yocto,
                100 * tgas
            )
        );
        assert_eq!(
            sent_call(
                token
                    .storage_deposit(account!("bob.near"), NearToken::from_millinear(5))
                    .await
                    .unwrap()
            ),
            (
                "storage_deposit".to_owned(),
                json!({"account_id": "bob.near", "registration_only": true}),
                NearToken::from_millinear(5).as_yoctonear(),
                30 * tgas
            )
        );
    }
}
