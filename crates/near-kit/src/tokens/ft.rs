//! Fungible token client (NEP-141).

use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::OnceCell;

use crate::client::{CallBuilder, RpcClient, Signer, TransactionBuilder};
use crate::error::Error;
use crate::types::{
    AccountId, Action, BlockReference, Finality, Gas, IntoNearToken, NearToken, Transaction,
    TxExecutionStatus,
};

use super::types::{FtAmount, FtMetadata, StorageBalance, StorageBalanceBounds};

// =============================================================================
// FungibleToken
// =============================================================================

/// Client for interacting with a NEP-141 Fungible Token contract.
///
/// Create via [`Near::ft()`](crate::Near::ft).
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
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::mainnet().build();
/// let usdc = near.ft(tokens::USDC)?;
///
/// // Get metadata
/// let meta = usdc.metadata().await?;
/// println!("{} has {} decimals", meta.symbol, meta.decimals);
///
/// // Get balance (returns FtAmount for nice formatting)
/// let balance = usdc.balance_of("alice.near").await?;
/// println!("Balance: {}", balance);  // "1.5 USDC"
/// # Ok(())
/// # }
/// ```
pub struct FungibleToken {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    contract_id: AccountId,
    metadata: OnceCell<FtMetadata>,
    storage_bounds: OnceCell<StorageBalanceBounds>,
}

impl FungibleToken {
    /// Create a new FungibleToken client.
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        contract_id: AccountId,
    ) -> Self {
        Self {
            rpc,
            signer,
            contract_id,
            metadata: OnceCell::new(),
            storage_bounds: OnceCell::new(),
        }
    }

    /// Get the contract ID.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }

    /// Create a transaction builder for this contract.
    fn transaction(&self) -> TransactionBuilder {
        TransactionBuilder::new(
            self.rpc.clone(),
            self.signer.clone(),
            self.contract_id.clone(),
        )
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
                let result = self
                    .rpc
                    .view_function(
                        &self.contract_id,
                        "ft_metadata",
                        &[],
                        BlockReference::Finality(Finality::Optimistic),
                    )
                    .await
                    .map_err(Error::from)?;
                result.json().map_err(Error::from)
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
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::mainnet().build();
    /// let usdc = near.ft(tokens::USDC)?;
    ///
    /// let balance = usdc.balance_of("alice.near").await?;
    /// println!("Balance: {}", balance);  // "1.5 USDC"
    /// println!("Raw: {}", balance.raw()); // 1500000
    /// # Ok(())
    /// # }
    /// ```
    pub async fn balance_of(&self, account_id: impl AsRef<str>) -> Result<FtAmount, Error> {
        let metadata = self.metadata().await?;

        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }

        let args = serde_json::to_vec(&Args {
            account_id: account_id.as_ref(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "ft_balance_of",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let balance_str: String = result.json().map_err(Error::from)?;
        let raw: u128 = balance_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid balance format: {}",
                balance_str
            )))
        })?;

        Ok(FtAmount::from_metadata(raw, metadata))
    }

    /// Get total token supply (ft_total_supply).
    ///
    /// Returns an [`FtAmount`] with the token's decimals and symbol.
    pub async fn total_supply(&self) -> Result<FtAmount, Error> {
        let metadata = self.metadata().await?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "ft_total_supply",
                &[],
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let supply_str: String = result.json().map_err(Error::from)?;
        let raw: u128 = supply_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            )))
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
    pub async fn is_registered(&self, account_id: impl AsRef<str>) -> Result<bool, Error> {
        let balance = self.storage_balance_of(account_id).await?;
        Ok(balance.is_some())
    }

    /// Get storage balance for an account (storage_balance_of).
    ///
    /// Returns `None` if the account is not registered.
    pub async fn storage_balance_of(
        &self,
        account_id: impl AsRef<str>,
    ) -> Result<Option<StorageBalance>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }

        let args = serde_json::to_vec(&Args {
            account_id: account_id.as_ref(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "storage_balance_of",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        result.json().map_err(Error::from)
    }

    /// Register an account on this token contract (storage_deposit).
    ///
    /// This deposits the minimum required NEAR to register the account for
    /// receiving tokens.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::mainnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let usdc = near.ft(tokens::USDC)?;
    ///
    /// // Register bob to receive USDC
    /// usdc.storage_deposit("bob.near").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn storage_deposit(&self, account_id: impl AsRef<str>) -> StorageDepositCall {
        StorageDepositCall::new(
            self.rpc.clone(),
            self.signer.clone(),
            self.contract_id.clone(),
            Some(account_id.as_ref().to_string()),
            self.storage_bounds.clone(),
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
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::mainnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let usdc = near.ft(tokens::USDC)?;
    ///
    /// // Transfer 1.5 USDC (raw amount for 6 decimals)
    /// usdc.transfer("bob.near", 1_500_000_u128).await?;
    ///
    /// // Or use an FtAmount from a query
    /// let balance = usdc.balance_of("alice.near").await?;
    /// usdc.transfer("bob.near", balance).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(&self, receiver_id: impl AsRef<str>, amount: impl Into<u128>) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            amount: String,
        }

        self.transaction()
            .call("ft_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                amount: amount.into().to_string(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Transfer tokens with a memo (ft_transfer).
    ///
    /// Same as [`transfer`](Self::transfer) but with an optional memo field.
    pub fn transfer_with_memo(
        &self,
        receiver_id: impl AsRef<str>,
        amount: impl Into<u128>,
        memo: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            amount: String,
            memo: String,
        }

        self.transaction()
            .call("ft_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                amount: amount.into().to_string(),
                memo: memo.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
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
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::mainnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let usdc = near.ft(tokens::USDC)?;
    ///
    /// // Deposit USDC into a DeFi contract
    /// usdc.transfer_call("defi.near", 1_000_000_u128, r#"{"action":"deposit"}"#)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl AsRef<str>,
        amount: impl Into<u128>,
        msg: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferCallArgs {
            receiver_id: String,
            amount: String,
            msg: String,
        }

        self.transaction()
            .call("ft_transfer_call")
            .args(TransferCallArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                amount: amount.into().to_string(),
                msg: msg.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(100))
    }
}

impl Clone for FungibleToken {
    fn clone(&self) -> Self {
        Self {
            rpc: self.rpc.clone(),
            signer: self.signer.clone(),
            contract_id: self.contract_id.clone(),
            metadata: OnceCell::new(),
            storage_bounds: OnceCell::new(),
        }
    }
}

impl std::fmt::Debug for FungibleToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FungibleToken")
            .field("contract_id", &self.contract_id)
            .field("metadata_cached", &self.metadata.initialized())
            .finish()
    }
}

// =============================================================================
// StorageDepositCall Builder
// =============================================================================

/// Builder for storage deposit transactions.
///
/// This builder needs special handling because it fetches storage bounds
/// to determine the deposit amount, which requires async prep work.
pub struct StorageDepositCall {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    contract_id: AccountId,
    account_id: Option<String>,
    deposit: Option<NearToken>,
    registration_only: bool,
    storage_bounds: OnceCell<StorageBalanceBounds>,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl StorageDepositCall {
    fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        contract_id: AccountId,
        account_id: Option<String>,
        storage_bounds: OnceCell<StorageBalanceBounds>,
    ) -> Self {
        Self {
            rpc,
            signer,
            contract_id,
            account_id,
            deposit: None,
            registration_only: true,
            storage_bounds,
            signer_override: None,
            wait_until: TxExecutionStatus::ExecutedOptimistic,
        }
    }

    /// Set a custom deposit amount (overrides automatic minimum).
    ///
    /// # Panics
    ///
    /// Panics if the amount string cannot be parsed. Use [`NearToken`]'s `FromStr`
    /// impl for fallible parsing of user input.
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        self.deposit = Some(
            amount
                .into_near_token()
                .expect("invalid deposit amount - use NearToken::from_str() for user input"),
        );
        self
    }

    /// Set registration_only flag (default: true).
    ///
    /// When true, any excess deposit is refunded. When false, excess is kept
    /// as additional storage deposit.
    pub fn registration_only(mut self, value: bool) -> Self {
        self.registration_only = value;
        self
    }

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
}

impl IntoFuture for StorageDepositCall {
    type Output = Result<StorageBalance, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let signer = self
                .signer_override
                .as_ref()
                .or(self.signer.as_ref())
                .ok_or(Error::NoSigner)?;

            let signer_id = signer.account_id().clone();

            // Determine deposit amount
            let deposit = if let Some(d) = self.deposit {
                d
            } else {
                // Fetch storage bounds to get minimum
                let bounds = self
                    .storage_bounds
                    .get_or_try_init(|| async {
                        let result = self
                            .rpc
                            .view_function(
                                &self.contract_id,
                                "storage_balance_bounds",
                                &[],
                                BlockReference::Finality(Finality::Optimistic),
                            )
                            .await
                            .map_err(Error::from)?;
                        result.json::<StorageBalanceBounds>().map_err(Error::from)
                    })
                    .await?;
                bounds.min
            };

            // Build args
            #[derive(Serialize)]
            struct DepositArgs {
                #[serde(skip_serializing_if = "Option::is_none")]
                account_id: Option<String>,
                #[serde(skip_serializing_if = "std::ops::Not::not")]
                registration_only: bool,
            }

            let args = serde_json::to_vec(&DepositArgs {
                account_id: self.account_id,
                registration_only: self.registration_only,
            })?;

            // Get a signing key atomically
            let key = signer.key();
            let public_key = key.public_key().clone();

            // Get access key for nonce
            let access_key = self
                .rpc
                .view_access_key(
                    &signer_id,
                    &public_key,
                    BlockReference::Finality(Finality::Optimistic),
                )
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
                access_key.nonce + 1,
                self.contract_id,
                block.header.hash,
                vec![Action::function_call(
                    "storage_deposit".to_string(),
                    args,
                    Gas::tgas(30),
                    deposit,
                )],
            );

            // Sign
            let signature = key.sign(tx.get_hash().as_bytes()).await?;
            let signed_tx = crate::types::SignedTransaction {
                transaction: tx,
                signature,
            };

            // Send
            let response = self.rpc.send_tx(&signed_tx, self.wait_until).await?;
            let outcome = response.outcome.ok_or_else(|| {
                Error::InvalidTransaction(format!(
                    "Transaction {} submitted with wait_until={:?} but no execution \
                     outcome was returned. Use rpc().send_tx() for fire-and-forget \
                     submission.",
                    response.transaction_hash, self.wait_until,
                ))
            })?;

            if outcome.is_failure() {
                return Err(Error::TransactionFailed(
                    outcome.failure_message().unwrap_or_default(),
                ));
            }

            // Parse return value
            let return_value = outcome
                .success_value()
                .ok_or_else(|| Error::TransactionFailed("No return value".to_string()))?;

            let storage_balance: StorageBalance = serde_json::from_slice(&return_value)?;
            Ok(storage_balance)
        })
    }
}
