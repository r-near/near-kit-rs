//! Non-fungible token client (NEP-171).

use std::sync::Arc;

use crate::trace::{self, Instrument};
use serde::Serialize;
use tokio::sync::OnceCell;

use crate::client::{CallBuilder, Near, Signer, TransactionBuilder};
use crate::error::Error;
use crate::types::{AccountId, Finality, Gas, NearToken, TryIntoAccountId};

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
#[derive(Clone)]
pub struct NonFungibleToken {
    near: Near,
    contract_id: AccountId,
    metadata: Arc<OnceCell<NftContractMetadata>>,
}

impl NonFungibleToken {
    /// Create a new NonFungibleToken client.
    pub(crate) fn new(near: Near, contract_id: AccountId) -> Self {
        Self {
            near,
            contract_id,
            metadata: Arc::new(OnceCell::new()),
        }
    }

    /// Get the contract ID.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }

    /// Create a new client with a different signer, sharing the same RPC connection.
    ///
    /// Cached metadata is shared with the original client.
    pub fn with_signer(&self, signer: impl Signer + 'static) -> Self {
        let mut token = self.clone();
        token.near = self.near.with_signer(signer);
        token
    }

    /// Create a transaction builder that surfaces argument validation errors
    /// when the builder is consumed.
    fn transaction_with_validation(&self, validation: Result<(), Error>) -> TransactionBuilder {
        self.near
            .transaction(&self.contract_id)
            .with_validation(validation)
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
                self.near
                    .view(&self.contract_id, "nft_metadata")
                    .finality(Finality::Optimistic)
                    .await
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
        let token_id = token_id.as_ref();
        let span = trace::debug_span!("nft_token", contract = %self.contract_id, token_id);

        async {
            #[derive(Serialize)]
            struct Args<'a> {
                token_id: &'a str,
            }

            self.near
                .view(&self.contract_id, "nft_token")
                .args(Args { token_id })
                .finality(Finality::Optimistic)
                .await
        }
        .instrument(span)
        .await
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
        account_id: impl TryIntoAccountId,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<NftToken>, Error> {
        let account_id: AccountId = account_id.try_into_account_id()?;
        let span =
            trace::debug_span!("nft_tokens_for_owner", contract = %self.contract_id, %account_id);

        async {
            #[derive(Serialize)]
            struct Args<'a> {
                account_id: &'a str,
                #[serde(skip_serializing_if = "Option::is_none")]
                from_index: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                limit: Option<u64>,
            }

            self.near
                .view(&self.contract_id, "nft_tokens_for_owner")
                .args(Args {
                    account_id: account_id.as_str(),
                    from_index: from_index.map(|i| i.to_string()),
                    limit,
                })
                .finality(Finality::Optimistic)
                .await
        }
        .instrument(span)
        .await
    }

    /// Get total supply of tokens (nft_total_supply).
    pub async fn total_supply(&self) -> Result<u64, Error> {
        let supply_str: String = self
            .near
            .view(&self.contract_id, "nft_total_supply")
            .finality(Finality::Optimistic)
            .await?;
        supply_str.parse().map_err(|_| {
            Error::Rpc(Box::new(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            ))))
        })
    }

    /// Get token supply for an owner (nft_supply_for_owner).
    pub async fn supply_for_owner(&self, account_id: impl TryIntoAccountId) -> Result<u64, Error> {
        let account_id: AccountId = account_id.try_into_account_id()?;

        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }

        let supply_str: String = self
            .near
            .view(&self.contract_id, "nft_supply_for_owner")
            .args(Args {
                account_id: account_id.as_str(),
            })
            .finality(Finality::Optimistic)
            .await?;
        supply_str.parse().map_err(|_| {
            Error::Rpc(Box::new(crate::error::RpcError::InvalidResponse(format!(
                "Invalid supply format: {}",
                supply_str
            ))))
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
    pub fn transfer(
        &self,
        receiver_id: impl TryIntoAccountId,
        token_id: impl AsRef<str>,
    ) -> CallBuilder {
        self.transfer_with_options(receiver_id, token_id, NftTransferOptions::default())
    }

    /// Transfer an NFT with a memo (nft_transfer).
    ///
    /// Same as [`transfer`](Self::transfer) but with an optional memo field.
    pub fn transfer_with_memo(
        &self,
        receiver_id: impl TryIntoAccountId,
        token_id: impl AsRef<str>,
        memo: impl Into<String>,
    ) -> CallBuilder {
        self.transfer_with_options(
            receiver_id,
            token_id,
            NftTransferOptions {
                memo: Some(memo.into()),
                ..Default::default()
            },
        )
    }

    /// Transfer an NFT with approval ID (for approved transfers).
    pub fn transfer_with_approval(
        &self,
        receiver_id: impl TryIntoAccountId,
        token_id: impl AsRef<str>,
        approval_id: u64,
    ) -> CallBuilder {
        self.transfer_with_options(
            receiver_id,
            token_id,
            NftTransferOptions {
                approval_id: Some(approval_id),
                ..Default::default()
            },
        )
    }

    fn transfer_with_options(
        &self,
        receiver_id: impl TryIntoAccountId,
        token_id: impl AsRef<str>,
        options: NftTransferOptions,
    ) -> CallBuilder {
        let (receiver_id, validation) = validate_account_id(receiver_id);
        trace::debug!(contract = %self.contract_id, token_id = token_id.as_ref(), receiver = %receiver_id, "nft_transfer");

        self.transaction_with_validation(validation)
            .call("nft_transfer")
            .args(NftTransferArgs {
                receiver_id,
                token_id: token_id.as_ref().to_string(),
                approval_id: options.approval_id,
                memo: options.memo,
            })
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(30))
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
        receiver_id: impl TryIntoAccountId,
        token_id: impl AsRef<str>,
        msg: impl Into<String>,
    ) -> CallBuilder {
        let (receiver_id, validation) = validate_account_id(receiver_id);
        trace::debug!(contract = %self.contract_id, token_id = token_id.as_ref(), receiver = %receiver_id, "nft_transfer_call");
        #[derive(Serialize)]
        struct TransferCallArgs {
            receiver_id: String,
            token_id: String,
            msg: String,
        }

        self.transaction_with_validation(validation)
            .call("nft_transfer_call")
            .args(TransferCallArgs {
                receiver_id,
                token_id: token_id.as_ref().to_string(),
                msg: msg.into(),
            })
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(100))
    }
}

#[derive(Default)]
struct NftTransferOptions {
    approval_id: Option<u64>,
    memo: Option<String>,
}

#[derive(Serialize)]
struct NftTransferArgs {
    receiver_id: String,
    token_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_id: Option<u64>,
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

impl std::fmt::Debug for NonFungibleToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonFungibleToken")
            .field("contract_id", &self.contract_id)
            .field("metadata_cached", &self.metadata.initialized())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, CryptoHash, StateInit, StateInitExt};

    fn token_without_signer() -> NonFungibleToken {
        NonFungibleToken::new(
            Near::custom("http://127.0.0.1:1", "test").build(),
            "nft.near".parse().unwrap(),
        )
    }

    fn call_args(call: CallBuilder) -> serde_json::Value {
        match call.into_action().unwrap() {
            Action::FunctionCall(action) => serde_json::from_slice(&action.args).unwrap(),
            action => panic!("expected function call, got {action:?}"),
        }
    }

    #[test]
    fn clones_share_metadata_cache() {
        let token = token_without_signer();
        token
            .metadata
            .set(NftContractMetadata {
                spec: "nft-1.0.0".to_string(),
                name: "Test Collection".to_string(),
                symbol: "TEST".to_string(),
                icon: None,
                base_uri: None,
                reference: None,
                reference_hash: None,
            })
            .unwrap();

        let cloned = token.clone();

        assert!(Arc::ptr_eq(&token.metadata, &cloned.metadata));
        assert_eq!(cloned.metadata.get().unwrap().symbol, "TEST");
    }

    #[test]
    fn transfer_helpers_preserve_nep_171_arguments() {
        let token = token_without_signer();

        assert_eq!(
            call_args(token.transfer("alice.near", "token-1")),
            serde_json::json!({"receiver_id": "alice.near", "token_id": "token-1"})
        );
        assert_eq!(
            call_args(token.transfer_with_memo("alice.near", "token-1", "memo")),
            serde_json::json!({
                "receiver_id": "alice.near",
                "token_id": "token-1",
                "memo": "memo"
            })
        );
        assert_eq!(
            call_args(token.transfer_with_approval("alice.near", "token-1", 7)),
            serde_json::json!({
                "receiver_id": "alice.near",
                "token_id": "token-1",
                "approval_id": 7
            })
        );
    }

    #[tokio::test]
    async fn call_methods_defer_invalid_account_ids() {
        let token = token_without_signer();
        let calls = [
            token.transfer("INVALID", "token-1"),
            token.transfer_with_memo("INVALID", "token-1", "memo"),
            token.transfer_with_approval("INVALID", "token-1", 1),
            token.transfer_call("INVALID", "token-1", "message"),
        ];

        for call in calls {
            assert!(matches!(call.await, Err(Error::ParseAccountId(_))));
        }
    }

    #[tokio::test]
    async fn valid_call_still_reaches_send_validation() {
        let error = token_without_signer()
            .transfer("alice.near", "token-1")
            .await
            .unwrap_err();

        assert!(matches!(error, Error::NoSigner));
    }

    #[tokio::test]
    async fn receiver_override_does_not_clear_invalid_account_id() {
        let error = token_without_signer()
            .transfer("INVALID", "token-1")
            .finish()
            .state_init(
                StateInit::by_hash(CryptoHash::ZERO, Default::default()),
                NearToken::ZERO,
            )
            .send()
            .await
            .unwrap_err();

        assert!(matches!(error, Error::ParseAccountId(_)));
    }
}
