//! Non-fungible token client (NEP-171).

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::OnceCell;

use crate::client::{CallBuilder, RpcClient, Signer, TransactionBuilder};
use crate::error::Error;
use crate::types::{AccountId, BlockReference, Finality, Gas, NearToken};

use super::types::{NftContractMetadata, NftToken};

// =============================================================================
// NonFungibleToken
// =============================================================================

/// Client for interacting with a NEP-171 Non-Fungible Token contract.
///
/// Create via [`Near::nft()`](crate::Near::nft).
///
/// # Caching
///
/// Contract metadata is lazily fetched and cached on first use.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
///
/// # async fn example() -> Result<(), near_kit::Error> {
/// let near = Near::testnet().build();
/// let nft = near.nft("nft-contract.near")?;
///
/// // Get contract metadata
/// let meta = nft.metadata().await.map_err(Error::from)?;
/// println!("Collection: {}", meta.name);
///
/// // Get a specific token
/// if let Some(token) = nft.token("token-123").await? {
///     println!("Owner: {}", token.owner_id);
/// }
///
/// // List tokens owned by an account
/// let tokens = nft.tokens_for_owner("alice.near", None, Some(10)).await.map_err(Error::from)?;
/// # Ok(())
/// # }
/// ```
pub struct NonFungibleToken {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    contract_id: AccountId,
    metadata: OnceCell<NftContractMetadata>,
}

impl NonFungibleToken {
    /// Create a new NonFungibleToken client.
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

    /// Get contract metadata (nft_metadata).
    ///
    /// Metadata is cached after the first call.
    pub async fn metadata(&self) -> Result<&NftContractMetadata, Error> {
        self.metadata
            .get_or_try_init(|| async {
                let result = self
                    .rpc
                    .view_function(
                        &self.contract_id,
                        "nft_metadata",
                        &[],
                        BlockReference::Finality(Finality::Optimistic),
                    )
                    .await
                    .map_err(Error::from)?;
                result.json().map_err(Error::from)
            })
            .await
    }

    /// Get a specific token by ID (nft_token).
    ///
    /// Returns `None` if the token doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let nft = near.nft("nft-contract.near")?;
    ///
    /// if let Some(token) = nft.token("token-123").await? {
    ///     println!("Token {} owned by {}", token.token_id, token.owner_id);
    ///     if let Some(meta) = &token.metadata {
    ///         println!("Title: {:?}", meta.title);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn token(&self, token_id: impl AsRef<str>) -> Result<Option<NftToken>, Error> {
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
                "nft_token",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        result.json().map_err(Error::from)
    }

    /// Get tokens owned by an account (nft_tokens_for_owner).
    ///
    /// Supports pagination via `from_index` and `limit`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet().build();
    /// let nft = near.nft("nft-contract.near")?;
    ///
    /// // Get first 10 tokens
    /// let tokens = nft.tokens_for_owner("alice.near", None, Some(10)).await?;
    /// for token in &tokens {
    ///     println!("Token: {}", token.token_id);
    /// }
    ///
    /// // Get next 10 tokens
    /// let more = nft.tokens_for_owner("alice.near", Some(10), Some(10)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn tokens_for_owner(
        &self,
        account_id: impl AsRef<str>,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<NftToken>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            from_index: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<u64>,
        }

        let args = serde_json::to_vec(&Args {
            account_id: account_id.as_ref(),
            from_index: from_index.map(|i| i.to_string()),
            limit,
        })?;

        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "nft_tokens_for_owner",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await?;

        result.json().map_err(Error::from)
    }

    /// Get total supply of tokens (nft_total_supply).
    pub async fn total_supply(&self) -> Result<u64, Error> {
        let result = self
            .rpc
            .view_function(
                &self.contract_id,
                "nft_total_supply",
                &[],
                BlockReference::Finality(Finality::Optimistic),
            )
            .await
            .map_err(Error::from)?;

        let supply_str: String = result.json()?;
        supply_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            )))
        })
    }

    /// Get token supply for an owner (nft_supply_for_owner).
    pub async fn supply_for_owner(&self, account_id: impl AsRef<str>) -> Result<u64, Error> {
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
                "nft_supply_for_owner",
                &args,
                BlockReference::Finality(Finality::Optimistic),
            )
            .await
            .map_err(Error::from)?;

        let supply_str: String = result.json()?;
        supply_str.parse().map_err(|_| {
            Error::Rpc(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            )))
        })
    }

    // =========================================================================
    // Transfer Methods
    // =========================================================================

    /// Transfer an NFT to a receiver (nft_transfer).
    ///
    /// # Security
    ///
    /// This automatically attaches 1 yoctoNEAR as required by NEP-171 for
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
    /// let nft = near.nft("nft-contract.near")?;
    ///
    /// nft.transfer("bob.near", "token-123").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(&self, receiver_id: impl AsRef<str>, token_id: impl AsRef<str>) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            token_id: String,
        }

        self.transaction()
            .call("nft_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                token_id: token_id.as_ref().to_string(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Transfer an NFT with a memo (nft_transfer).
    ///
    /// Same as [`transfer`](Self::transfer) but with an optional memo field.
    pub fn transfer_with_memo(
        &self,
        receiver_id: impl AsRef<str>,
        token_id: impl AsRef<str>,
        memo: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            token_id: String,
            memo: String,
        }

        self.transaction()
            .call("nft_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                token_id: token_id.as_ref().to_string(),
                memo: memo.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Transfer an NFT with approval ID (for approved transfers).
    pub fn transfer_with_approval(
        &self,
        receiver_id: impl AsRef<str>,
        token_id: impl AsRef<str>,
        approval_id: u64,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferArgs {
            receiver_id: String,
            token_id: String,
            approval_id: u64,
        }

        self.transaction()
            .call("nft_transfer")
            .args(TransferArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                token_id: token_id.as_ref().to_string(),
                approval_id,
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(30))
    }

    /// Transfer an NFT with a callback to the receiver (nft_transfer_call).
    ///
    /// This calls `nft_on_transfer` on the receiver contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # async fn example() -> Result<(), near_kit::Error> {
    /// let near = Near::testnet()
    ///     .credentials("ed25519:...", "alice.near")?
    ///     .build();
    /// let nft = near.nft("nft-contract.near")?;
    ///
    /// nft.transfer_call("marketplace.near", "token-123", r#"{"action":"list","price":"10"}"#)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl AsRef<str>,
        token_id: impl AsRef<str>,
        msg: impl Into<String>,
    ) -> CallBuilder {
        #[derive(Serialize)]
        struct TransferCallArgs {
            receiver_id: String,
            token_id: String,
            msg: String,
        }

        self.transaction()
            .call("nft_transfer_call")
            .args(TransferCallArgs {
                receiver_id: receiver_id.as_ref().to_string(),
                token_id: token_id.as_ref().to_string(),
                msg: msg.into(),
            })
            .deposit(NearToken::yocto(1))
            .gas(Gas::tgas(100))
    }
}

impl Clone for NonFungibleToken {
    fn clone(&self) -> Self {
        Self {
            rpc: self.rpc.clone(),
            signer: self.signer.clone(),
            contract_id: self.contract_id.clone(),
            metadata: OnceCell::new(),
        }
    }
}

impl std::fmt::Debug for NonFungibleToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonFungibleToken")
            .field("contract_id", &self.contract_id)
            .field("metadata_cached", &self.metadata.initialized())
            .finish()
    }
}
