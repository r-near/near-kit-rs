//! Multi token client (NEP-245).

use std::sync::Arc;

use serde::Serialize;

use crate::client::{CallBuilder, RpcClient, Signer, TransactionBuilder};
use crate::error::Error;
use crate::types::{AccountId, BlockReference, Finality, Gas, NearToken};

use super::types::MtToken;

// =============================================================================
// MultiToken
// =============================================================================

/// Client for interacting with a NEP-245 Multi Token contract.
///
/// Create via [`Near::mt()`](crate::Near::mt).
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
/// let mt = near.mt("mt-contract.near")?;
///
/// // Get a specific token
/// if let Some(token) = mt.token("token-1").await? {
///     println!("Token: {:?}", token);
/// }
///
/// // Get balance of a specific token for an account
/// let balance = mt.balance_of("alice.near", "token-1").await?;
/// println!("Balance: {}", balance);
///
/// // Get total supply of a specific token
/// let supply = mt.supply("token-1").await?;
/// println!("Supply: {}", supply);
/// # Ok(())
/// # }
/// ```
pub struct MultiToken {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    contract_id: AccountId,
    max_nonce_retries: u32,
}

impl MultiToken {
    /// Create a new MultiToken client.
    pub(crate) fn new(
        rpc: Arc<RpcClient>,
        signer: Option<Arc<dyn Signer>>,
        contract_id: AccountId,
        max_nonce_retries: u32,
    ) -> Self {
        Self {
            rpc,
            signer,
            contract_id,
            max_nonce_retries,
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
            self.max_nonce_retries,
        )
    }

    // =========================================================================
    // View Methods
    // =========================================================================

    /// Get a specific token by ID (mt_token).
    ///
    /// Returns `None` if the token doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// if let Some(token) = mt.token("token-1").await? {
    ///     println!("Token: {:?}", token);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn token(&self, token_id: impl AsRef<str>) -> Result<Option<MtToken>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            token_ids: Vec<&'a str>,
        }

        let args = serde_json::to_vec(&Args {
            token_ids: vec![token_id.as_ref()],
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_token",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let tokens: Vec<Option<MtToken>> = result.json().map_err(Error::from)?;
        Ok(tokens.into_iter().next().flatten())
    }

    /// Get info for multiple tokens by ID (mt_batch_tokens).
    ///
    /// Returns a vector of optional tokens (None for IDs that don't exist).
    pub async fn batch_tokens(
        &self,
        token_ids: &[impl AsRef<str>],
    ) -> Result<Vec<Option<MtToken>>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            token_ids: Vec<&'a str>,
        }

        let args = serde_json::to_vec(&Args {
            token_ids: token_ids.iter().map(|id| id.as_ref()).collect(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_token",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        result.json().map_err(Error::from)
    }

    /// Get the balance of a specific token for an account (mt_balance_of).
    ///
    /// Returns the balance as a string (u128 encoded as string per NEP-245).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// let balance = mt.balance_of("alice.near", "token-1").await?;
    /// println!("Balance: {}", balance);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn balance_of(
        &self,
        account_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
    ) -> Result<u128, Error> {
        let account_id: AccountId = account_id.into();

        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
            token_id: &'a str,
        }

        let args = serde_json::to_vec(&Args {
            account_id: account_id.as_str(),
            token_id: token_id.as_ref(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_balance_of",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let balance_str: String = result.json().map_err(Error::from)?;
        balance_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid balance format: {}",
                balance_str
            )))
        })
    }

    /// Get balances for multiple token/account pairs (mt_batch_balance_of).
    ///
    /// Each element in `account_ids` and `token_ids` must correspond pairwise.
    /// Returns a vector of balances in the same order.
    pub async fn batch_balance_of(
        &self,
        account_ids: &[impl AsRef<str>],
        token_ids: &[impl AsRef<str>],
    ) -> Result<Vec<u128>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_ids: Vec<&'a str>,
            token_ids: Vec<&'a str>,
        }

        let args = serde_json::to_vec(&Args {
            account_ids: account_ids.iter().map(|id| id.as_ref()).collect(),
            token_ids: token_ids.iter().map(|id| id.as_ref()).collect(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_batch_balance_of",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let balances: Vec<String> = result.json().map_err(Error::from)?;
        balances
            .into_iter()
            .map(|s| {
                s.parse().map_err(|_| {
                    Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                        "Invalid balance format: {}",
                        s
                    )))
                })
            })
            .collect()
    }

    /// Get the total supply of a specific token (mt_supply).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// let supply = mt.supply("token-1").await?;
    /// println!("Supply: {}", supply);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn supply(&self, token_id: impl AsRef<str>) -> Result<u128, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            token_id: &'a str,
        }

        let args = serde_json::to_vec(&Args {
            token_id: token_id.as_ref(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_supply",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let supply_str: String = result.json().map_err(Error::from)?;
        supply_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            )))
        })
    }

    /// Get total supplies for multiple tokens (mt_batch_supply).
    ///
    /// Returns supplies in the same order as the provided token IDs.
    pub async fn batch_supply(&self, token_ids: &[impl AsRef<str>]) -> Result<Vec<u128>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            token_ids: Vec<&'a str>,
        }

        let args = serde_json::to_vec(&Args {
            token_ids: token_ids.iter().map(|id| id.as_ref()).collect(),
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "mt_batch_supply",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        let supplies: Vec<String> = result.json().map_err(Error::from)?;
        supplies
            .into_iter()
            .map(|s| {
                s.parse().map_err(|_| {
                    Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                        "Invalid supply format: {}",
                        s
                    )))
                })
            })
            .collect()
    }

    // =========================================================================
    // Transfer Methods
    // =========================================================================

    /// Transfer tokens to a receiver (mt_transfer).
    ///
    /// # Security
    ///
    /// This automatically attaches 1 yoctoNEAR as required by NEP-245 for
    /// security (prevents function-call access key abuse).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// mt.transfer("bob.near", "token-1", 100_u128).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        amount: impl Into<u128>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            token_id: String,
            amount: String,
        }

        self.transaction()
            .call("mt_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.into().to_string(),
                token_id: token_id.as_ref().to_string(),
                amount: amount.into().to_string(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Transfer tokens with a memo (mt_transfer).
    ///
    /// Same as [`transfer`](Self::transfer) but with an optional memo field.
    pub fn transfer_with_memo(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        amount: impl Into<u128>,
        memo: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            token_id: String,
            amount: String,
            memo: String,
        }

        self.transaction()
            .call("mt_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.into().to_string(),
                token_id: token_id.as_ref().to_string(),
                amount: amount.into().to_string(),
                memo: memo.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Batch transfer multiple token types (mt_batch_transfer).
    ///
    /// Transfers multiple token types in a single transaction.
    /// Each element in `token_ids` and `amounts` must correspond pairwise.
    ///
    /// # Security
    ///
    /// This automatically attaches 1 yoctoNEAR as required by NEP-245.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// mt.batch_transfer(
    ///     "bob.near",
    ///     &["token-1", "token-2"],
    ///     &[100_u128, 200_u128],
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn batch_transfer(
        &self,
        receiver_id: impl Into<AccountId>,
        token_ids: &[impl AsRef<str>],
        amounts: &[u128],
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct BatchTransferArgs {
            receiver_id: String,
            token_ids: Vec<String>,
            amounts: Vec<String>,
        }

        self.transaction()
            .call("mt_batch_transfer")
            .args(BatchTransferArgs {
                receiver_id: receiver_id.into().to_string(),
                token_ids: token_ids.iter().map(|id| id.as_ref().to_string()).collect(),
                amounts: amounts.iter().map(|a| a.to_string()).collect(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(60))
    }

    /// Transfer tokens with a callback to the receiver (mt_transfer_call).
    ///
    /// This calls `mt_on_transfer` on the receiver contract, allowing it to
    /// handle the tokens. The receiver can return unused tokens, which will
    /// be refunded to the sender.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// mt.transfer_call("defi.near", "token-1", 100_u128, r#"{"action":"deposit"}"#)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        amount: impl Into<u128>,
        msg: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferCallArgs {
            receiver_id: String,
            token_id: String,
            amount: String,
            msg: String,
        }

        self.transaction()
            .call("mt_transfer_call")
            .args(TransferCallArgs {
                receiver_id: receiver_id.into().to_string(),
                token_id: token_id.as_ref().to_string(),
                amount: amount.into().to_string(),
                msg: msg.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(100))
    }

    /// Batch transfer with a callback to the receiver (mt_batch_transfer_call).
    ///
    /// This calls `mt_on_transfer` on the receiver contract with multiple
    /// token types. Each element in `token_ids` and `amounts` must correspond
    /// pairwise.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let mt = near.mt("mt-contract.near")?;
    ///
    /// mt.batch_transfer_call(
    ///     "defi.near",
    ///     &["token-1", "token-2"],
    ///     &[100_u128, 200_u128],
    ///     r#"{"action":"deposit"}"#,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn batch_transfer_call(
        &self,
        receiver_id: impl Into<AccountId>,
        token_ids: &[impl AsRef<str>],
        amounts: &[u128],
        msg: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct BatchTransferCallArgs {
            receiver_id: String,
            token_ids: Vec<String>,
            amounts: Vec<String>,
            msg: String,
        }

        self.transaction()
            .call("mt_batch_transfer_call")
            .args(BatchTransferCallArgs {
                receiver_id: receiver_id.into().to_string(),
                token_ids: token_ids.iter().map(|id| id.as_ref().to_string()).collect(),
                amounts: amounts.iter().map(|a| a.to_string()).collect(),
                msg: msg.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(100))
    }
}

impl Clone for MultiToken {
    fn clone(&self) -> Self {
        Self {
            rpc: self.rpc.clone(),
            signer: self.signer.clone(),
            contract_id: self.contract_id.clone(),
            max_nonce_retries: self.max_nonce_retries,
        }
    }
}

impl std::fmt::Debug for MultiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiToken")
            .field("contract_id", &self.contract_id)
            .finish()
    }
}
