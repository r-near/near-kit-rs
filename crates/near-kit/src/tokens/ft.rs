//! Fungible token client (NEP-141).

use std::sync::Arc;

use crate::trace::{self, Instrument};
use serde::Serialize;
use tokio::sync::OnceCell;

use crate::client::{SigningNear, Tx};
use crate::error::Error;
use crate::transaction::FunctionCall;
use crate::types::{
    AccountId, ExecutedOptimistic, Finality, Gas, NearToken, TryIntoAccountId, WaitLevel,
};

use super::types::{FtAmount, FtMetadata, StorageBalance, StorageBalanceBounds};

// =============================================================================
// FungibleToken
// =============================================================================

/// Client for interacting with a NEP-141 Fungible Token contract.
///
/// Create via [`SigningNear::ft()`](crate::SigningNear::ft). It reads through
/// the client's [`Near`](crate::Near) and sends its calls as the client's
/// account, waiting for the client's wait level `W`.
///
/// # Caching
///
/// Token metadata is lazily fetched and cached on first use. Subsequent calls
/// to methods that need metadata (like `balance_of`) will use the cached value.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
///
/// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
/// let token = near.ft("wrap.near".parse::<AccountId>()?);
///
/// // Get metadata
/// let meta = token.metadata().await?;
/// println!("{} has {} decimals", meta.symbol, meta.decimals);
///
/// // Get balance (returns FtAmount for nice formatting)
/// let balance = token.balance_of("alice.near").await?;
/// println!("Balance: {}", balance);
/// # Ok(())
/// # }
/// ```
pub struct FungibleToken<W: WaitLevel = ExecutedOptimistic> {
    client: SigningNear<W>,
    contract_id: AccountId,
    metadata: Arc<OnceCell<FtMetadata>>,
    storage_bounds: Arc<OnceCell<StorageBalanceBounds>>,
}

impl<W: WaitLevel> FungibleToken<W> {
    /// Create a new FungibleToken client.
    pub(crate) fn new(client: SigningNear<W>, contract_id: AccountId) -> Self {
        Self {
            client,
            contract_id,
            metadata: Arc::new(OnceCell::new()),
            storage_bounds: Arc::new(OnceCell::new()),
        }
    }

    /// Get the contract ID.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }

    /// A transaction to this contract carrying `call`, keeping an argument
    /// validation error to return from `.await`.
    fn call(&self, validation: Result<(), Error>, call: FunctionCall) -> Tx<W> {
        let mut tx = self.client.transaction(&self.contract_id);
        if let Err(error) = validation {
            tx.defer(error);
        }
        tx.add_action(call)
    }

    // =========================================================================
    // View Methods
    // =========================================================================

    /// Get token metadata (ft_metadata).
    ///
    /// Metadata is cached after the first call.
    pub async fn metadata(&self) -> Result<&FtMetadata, Error> {
        self.metadata
            .get_or_try_init(|| async {
                self.client
                    .near()
                    .view(&self.contract_id, "ft_metadata")
                    .finality(Finality::Optimistic)
                    .await
            })
            .await
    }

    /// Get token balance for an account (ft_balance_of).
    ///
    /// Returns an [`FtAmount`] with the token's decimals and symbol for easy formatting.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = near.ft("wrap.near".parse::<AccountId>()?);
    ///
    /// let balance = token.balance_of("alice.near").await?;
    /// println!("Balance: {}", balance);
    /// println!("Raw: {}", balance.raw());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn balance_of(&self, account_id: impl TryIntoAccountId) -> Result<FtAmount, Error> {
        let account_id: AccountId = account_id.try_into_account_id()?;
        let span = trace::debug_span!("ft_balance_of", contract = %self.contract_id, %account_id);

        async {
            let metadata = self.metadata().await?;

            #[derive(Serialize)]
            struct Args<'a> {
                account_id: &'a str,
            }

            let balance_str: String = self
                .client
                .near()
                .view(&self.contract_id, "ft_balance_of")
                .args(Args {
                    account_id: account_id.as_str(),
                })
                .finality(Finality::Optimistic)
                .await?;
            let raw: u128 = balance_str.parse().map_err(|_| {
                Error::Rpc(Box::new(crate::error::RpcError::InvalidResponse(format!(
                    "Invalid balance format: {}",
                    balance_str
                ))))
            })?;

            Ok(FtAmount::from_metadata(raw, metadata))
        }
        .instrument(span)
        .await
    }

    /// Get total token supply (ft_total_supply).
    ///
    /// Returns an [`FtAmount`] with the token's decimals and symbol.
    pub async fn total_supply(&self) -> Result<FtAmount, Error> {
        let metadata = self.metadata().await?;

        let supply_str: String = self
            .client
            .near()
            .view(&self.contract_id, "ft_total_supply")
            .finality(Finality::Optimistic)
            .await?;
        let raw: u128 = supply_str.parse().map_err(|_| {
            Error::Rpc(Box::new(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            ))))
        })?;

        Ok(FtAmount::from_metadata(raw, metadata))
    }

    // =========================================================================
    // Storage Methods (NEP-145)
    // =========================================================================

    /// Check if an account is registered on this token contract.
    ///
    /// An account must be registered (via `storage_deposit`) before it can
    /// receive tokens.
    pub async fn is_registered(&self, account_id: impl TryIntoAccountId) -> Result<bool, Error> {
        let balance = self.storage_balance_of(account_id).await?;
        Ok(balance.is_some())
    }

    /// Get storage balance for an account (storage_balance_of).
    ///
    /// Returns `None` if the account is not registered.
    pub async fn storage_balance_of(
        &self,
        account_id: impl TryIntoAccountId,
    ) -> Result<Option<StorageBalance>, Error> {
        let account_id: AccountId = account_id.try_into_account_id()?;

        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }

        self.client
            .near()
            .view(&self.contract_id, "storage_balance_of")
            .args(Args {
                account_id: account_id.as_str(),
            })
            .finality(Finality::Optimistic)
            .await
    }

    /// Get storage balance bounds for this token contract.
    ///
    /// Returns the minimum and maximum storage deposit amounts.
    /// The minimum is typically needed for [`storage_deposit`](Self::storage_deposit).
    pub async fn storage_balance_bounds(&self) -> Result<&StorageBalanceBounds, Error> {
        self.storage_bounds
            .get_or_try_init(|| async {
                self.client
                    .near()
                    .view(&self.contract_id, "storage_balance_bounds")
                    .finality(Finality::Optimistic)
                    .await
            })
            .await
    }

    /// Register an account on this token contract (storage_deposit).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = near.ft("wrap.near".parse::<AccountId>()?);
    ///
    /// // Register bob with auto-detected minimum deposit
    /// let bounds = token.storage_balance_bounds().await?;
    /// token.storage_deposit("bob.near", bounds.min).await?;
    ///
    /// // Or with a known amount
    /// token.storage_deposit("bob.near", NearToken::from_millinear(50)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn storage_deposit(&self, account_id: impl TryIntoAccountId, deposit: NearToken) -> Tx<W> {
        let (account_id, validation) = validate_account_id(account_id);

        #[derive(Serialize)]
        struct DepositArgs {
            account_id: String,
            registration_only: bool,
        }

        self.call(
            validation,
            FunctionCall::new("storage_deposit")
                .args(DepositArgs {
                    account_id,
                    registration_only: true,
                })
                .deposit(deposit)
                .gas(Gas::from_tgas(30)),
        )
    }

    // =========================================================================
    // Transfer Methods
    // =========================================================================

    /// Transfer tokens to a receiver (ft_transfer).
    ///
    /// Amount is in raw token units. Use [`FtAmount`] from a previous query,
    /// or specify the raw value directly.
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
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = near.ft("wrap.near".parse::<AccountId>()?);
    ///
    /// token.transfer("bob.near", 1_500_000_u128).await?;
    ///
    /// // Or use an FtAmount from a query
    /// let balance = token.balance_of("alice.near").await?;
    /// token.transfer("bob.near", balance).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(&self, receiver_id: impl TryIntoAccountId, amount: impl Into<u128>) -> Tx<W> {
        self.transfer_with_optional_memo(receiver_id, amount.into(), None)
    }

    /// Transfer tokens with a memo (ft_transfer).
    ///
    /// Same as [`transfer`](Self::transfer) but with an optional memo field.
    pub fn transfer_with_memo(
        &self,
        receiver_id: impl TryIntoAccountId,
        amount: impl Into<u128>,
        memo: impl Into<String>,
    ) -> Tx<W> {
        self.transfer_with_optional_memo(receiver_id, amount.into(), Some(memo.into()))
    }

    fn transfer_with_optional_memo(
        &self,
        receiver_id: impl TryIntoAccountId,
        amount: u128,
        memo: Option<String>,
    ) -> Tx<W> {
        let (receiver_id, validation) = validate_account_id(receiver_id);
        trace::debug!(contract = %self.contract_id, receiver = %receiver_id, "ft_transfer");

        self.call(
            validation,
            FunctionCall::new("ft_transfer")
                .args(FtTransferArgs {
                    receiver_id,
                    amount: amount.to_string(),
                    memo,
                })
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
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let token = near.ft("wrap.near".parse::<AccountId>()?);
    ///
    /// token.transfer_call("defi.near", 1_000_000_u128, r#"{"action":"deposit"}"#)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl TryIntoAccountId,
        amount: impl Into<u128>,
        msg: impl Into<String>,
    ) -> Tx<W> {
        let (receiver_id, validation) = validate_account_id(receiver_id);
        trace::debug!(contract = %self.contract_id, receiver = %receiver_id, "ft_transfer_call");

        #[derive(Serialize)]
        struct TransferCallArgs {
            receiver_id: String,
            amount: String,
            msg: String,
        }

        self.call(
            validation,
            FunctionCall::new("ft_transfer_call")
                .args(TransferCallArgs {
                    receiver_id,
                    amount: amount.into().to_string(),
                    msg: msg.into(),
                })
                .deposit(NearToken::from_yoctonear(1))
                .gas(Gas::from_tgas(100)),
        )
    }
}

#[derive(Serialize)]
struct FtTransferArgs {
    receiver_id: String,
    amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    memo: Option<String>,
}

fn validate_account_id(account_id: impl TryIntoAccountId) -> (String, Result<(), Error>) {
    let original = account_id.as_str().to_owned();
    match account_id.try_into_account_id() {
        Ok(account_id) => (account_id.to_string(), Ok(())),
        Err(error) => (original, Err(error.into())),
    }
}

impl<W: WaitLevel> Clone for FungibleToken<W> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            contract_id: self.contract_id.clone(),
            metadata: self.metadata.clone(),
            storage_bounds: self.storage_bounds.clone(),
        }
    }
}

impl<W: WaitLevel> std::fmt::Debug for FungibleToken<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FungibleToken")
            .field("account_id", self.client.account_id())
            .field("contract_id", &self.contract_id)
            .field("metadata_cached", &self.metadata.initialized())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, InMemorySigner, SecretKey};

    fn token_without_signer() -> FungibleToken {
        FungibleToken::new(
            crate::Near::custom("http://127.0.0.1:1", "test")
                .build()
                .with_signer(
                    InMemorySigner::from_secret_key("alice.near", SecretKey::generate_ed25519())
                        .unwrap(),
                ),
            "token.near".parse().unwrap(),
        )
    }

    fn call_args(call: Tx) -> serde_json::Value {
        assert!(call.error.is_none(), "{:?}", call.error);
        assert_eq!(call.actions.len(), 1);
        match call.actions.into_iter().next().unwrap() {
            Action::FunctionCall(action) => serde_json::from_slice(&action.args).unwrap(),
            action => panic!("expected function call, got {action:?}"),
        }
    }

    #[test]
    fn clones_share_metadata_and_storage_caches() {
        let token = token_without_signer();
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
    fn transfer_helpers_preserve_nep_141_arguments() {
        let token = token_without_signer();

        assert_eq!(
            call_args(token.transfer("alice.near", 7_u128)),
            serde_json::json!({"receiver_id": "alice.near", "amount": "7"})
        );
        assert_eq!(
            call_args(token.transfer_with_memo("alice.near", 7_u128, "memo")),
            serde_json::json!({
                "receiver_id": "alice.near",
                "amount": "7",
                "memo": "memo"
            })
        );
    }

    #[tokio::test]
    async fn call_methods_defer_invalid_account_ids() {
        let token = token_without_signer();
        let calls = [
            token.storage_deposit("INVALID", NearToken::ZERO),
            token.transfer("INVALID", 1_u128),
            token.transfer_with_memo("INVALID", 1_u128, "memo"),
            token.transfer_call("INVALID", 1_u128, "message"),
        ];

        for call in calls {
            assert!(matches!(call.await, Err(Error::ParseAccountId(_))));
        }
    }
}
