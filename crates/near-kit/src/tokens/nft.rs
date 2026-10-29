//! Non-fungible token client (NEP-171).

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

use super::types::{NftContractMetadata, NftToken};
use super::{Stringified, read};

// =============================================================================
// NonFungibleToken
// =============================================================================

/// A NEP-171 non-fungible token contract.
///
/// The marker for [`Near::contract`](crate::Near::contract) and
/// [`SigningNear::contract`](crate::SigningNear::contract), like the unit
/// struct `#[contract]` generates. Its client is [`NonFungibleTokenClient`];
/// for another backend, use [`Contract::client`].
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::*;
/// use near_kit::standards::NonFungibleToken;
///
/// # async fn example(near: Near) -> Result<(), near_kit::Error> {
/// let nft = near.contract::<NonFungibleToken>(account!("nft-contract.near"));
///
/// // Get contract metadata
/// let meta = nft.metadata().await?;
/// println!("Collection: {}", meta.name);
///
/// // Get a specific token
/// if let Some(token) = nft.token("token-123").await? {
///     println!("Owner: {}", token.owner_id);
/// }
///
/// // List tokens owned by an account
/// let tokens = nft.tokens_for_owner(account!("alice.near"), None, Some(10)).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct NonFungibleToken;

/// Client for a NEP-171 non-fungible token contract, over a backend `C`.
///
/// Views need `C: Viewer` and read at optimistic finality; calls need
/// `C: TxSender + Clone` and return a [`Call`] that sends one transaction
/// when awaited. `near.contract::<NonFungibleToken>(id)` gives a read-only
/// client and `signing.contract::<NonFungibleToken>(id)` one that can also
/// send.
///
/// # Caching
///
/// Contract metadata is fetched on first use and cached. Clones share the
/// cache.
pub struct NonFungibleTokenClient<C = Near> {
    backend: C,
    contract_id: AccountId,
    metadata: Arc<OnceCell<NftContractMetadata>>,
}

impl Contract for NonFungibleToken {
    type Client<C> = NonFungibleTokenClient<C>;

    fn client<C>(backend: C, contract_id: AccountId) -> NonFungibleTokenClient<C> {
        NonFungibleTokenClient {
            backend,
            contract_id,
            metadata: Arc::new(OnceCell::new()),
        }
    }
}

impl<C> NonFungibleTokenClient<C> {
    /// Get the contract ID.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }
}

// =============================================================================
// View Methods
// =============================================================================

impl<C: Viewer> NonFungibleTokenClient<C> {
    /// Get contract metadata (nft_metadata).
    ///
    /// Metadata is cached after the first call.
    pub async fn metadata(&self) -> Result<&NftContractMetadata, ContractError<C::ViewError>> {
        self.metadata
            .get_or_try_init(|| read(&self.backend, &self.contract_id, "nft_metadata", None))
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
    /// # use near_kit::standards::NonFungibleToken;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let nft = near.contract::<NonFungibleToken>(account!("nft-contract.near"));
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
    pub async fn token(
        &self,
        token_id: impl AsRef<str>,
    ) -> Result<Option<NftToken>, ContractError<C::ViewError>> {
        let token_id = token_id.as_ref();
        let span = trace::debug_span!("nft_token", contract = %self.contract_id, token_id);

        read(
            &self.backend,
            &self.contract_id,
            "nft_token",
            Some(json!({ "token_id": token_id })),
        )
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
    /// # use near_kit::standards::NonFungibleToken;
    /// # async fn example(near: Near) -> Result<(), near_kit::Error> {
    /// let nft = near.contract::<NonFungibleToken>(account!("nft-contract.near"));
    ///
    /// // Get first 10 tokens
    /// let tokens = nft.tokens_for_owner(account!("alice.near"), None, Some(10)).await?;
    /// for token in &tokens {
    ///     println!("Token: {}", token.token_id);
    /// }
    ///
    /// // Get next 10 tokens
    /// let more = nft
    ///     .tokens_for_owner(account!("alice.near"), Some(10), Some(10))
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn tokens_for_owner(
        &self,
        account_id: impl Into<AccountId>,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<NftToken>, ContractError<C::ViewError>> {
        let account_id = account_id.into();
        let span =
            trace::debug_span!("nft_tokens_for_owner", contract = %self.contract_id, %account_id);

        let mut args = json!({ "account_id": account_id });
        if let Some(from_index) = from_index {
            args["from_index"] = from_index.to_string().into();
        }
        if let Some(limit) = limit {
            args["limit"] = limit.into();
        }
        read(
            &self.backend,
            &self.contract_id,
            "nft_tokens_for_owner",
            Some(args),
        )
        .instrument(span)
        .await
    }

    /// Get total supply of tokens (nft_total_supply).
    pub async fn total_supply(&self) -> Result<u64, ContractError<C::ViewError>> {
        let Stringified(supply) =
            read(&self.backend, &self.contract_id, "nft_total_supply", None).await?;
        Ok(supply)
    }

    /// Get token supply for an owner (nft_supply_for_owner).
    pub async fn supply_for_owner(
        &self,
        account_id: impl Into<AccountId>,
    ) -> Result<u64, ContractError<C::ViewError>> {
        let account_id = account_id.into();
        let Stringified(supply) = read(
            &self.backend,
            &self.contract_id,
            "nft_supply_for_owner",
            Some(json!({ "account_id": account_id })),
        )
        .await?;
        Ok(supply)
    }
}

// =============================================================================
// Call Methods
// =============================================================================

impl<C: TxSender + Clone> NonFungibleTokenClient<C> {
    /// Transfer an NFT to a receiver (nft_transfer), with an optional memo.
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
    /// # use near_kit::standards::NonFungibleToken;
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let nft = near.contract::<NonFungibleToken>(account!("nft-contract.near"));
    ///
    /// nft.transfer(account!("bob.near"), "token-123", None).await?;
    /// nft.transfer(account!("bob.near"), "token-456", Some("gift".into()))
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        memo: Option<String>,
    ) -> Call<C> {
        self.nft_transfer(receiver_id.into(), token_id.as_ref(), None, memo)
    }

    /// Transfer an NFT as an approved account (nft_transfer with
    /// `approval_id`).
    pub fn transfer_with_approval(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        approval_id: u64,
    ) -> Call<C> {
        self.nft_transfer(
            receiver_id.into(),
            token_id.as_ref(),
            Some(approval_id),
            None,
        )
    }

    fn nft_transfer(
        &self,
        receiver_id: AccountId,
        token_id: &str,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) -> Call<C> {
        trace::debug!(contract = %self.contract_id, token_id, receiver = %receiver_id, "nft_transfer");

        let mut args = json!({ "receiver_id": receiver_id, "token_id": token_id });
        if let Some(approval_id) = approval_id {
            args["approval_id"] = approval_id.into();
        }
        if let Some(memo) = memo {
            args["memo"] = memo.into();
        }
        call_through(
            self.backend.clone(),
            self.contract_id.clone(),
            FunctionCall::new("nft_transfer")
                .args(args)
                .deposit(NearToken::from_yoctonear(1))
                .gas(Gas::from_tgas(30)),
        )
    }

    /// Transfer an NFT with a callback to the receiver (nft_transfer_call).
    ///
    /// This calls `nft_on_transfer` on the receiver contract.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # use near_kit::standards::NonFungibleToken;
    /// # async fn example(near: SigningNear) -> Result<(), near_kit::Error> {
    /// let nft = near.contract::<NonFungibleToken>(account!("nft-contract.near"));
    ///
    /// nft.transfer_call(
    ///     account!("marketplace.near"),
    ///     "token-123",
    ///     r#"{"action":"list","price":"10"}"#,
    /// )
    /// .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transfer_call(
        &self,
        receiver_id: impl Into<AccountId>,
        token_id: impl AsRef<str>,
        msg: impl Into<String>,
    ) -> Call<C> {
        let receiver_id = receiver_id.into();
        let token_id = token_id.as_ref();
        trace::debug!(contract = %self.contract_id, token_id, receiver = %receiver_id, "nft_transfer_call");

        call_through(
            self.backend.clone(),
            self.contract_id.clone(),
            FunctionCall::new("nft_transfer_call")
                .args(json!({
                    "receiver_id": receiver_id,
                    "token_id": token_id,
                    "msg": msg.into(),
                }))
                .deposit(NearToken::from_yoctonear(1))
                .gas(Gas::from_tgas(100)),
        )
    }
}

impl<C: Clone> Clone for NonFungibleTokenClient<C> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            contract_id: self.contract_id.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl<C> std::fmt::Debug for NonFungibleTokenClient<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NonFungibleTokenClient")
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

    #[test]
    fn clones_share_metadata_cache() {
        let token = NonFungibleToken::client(Mock::default(), account!("nft.near"));
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

    #[tokio::test]
    async fn views_send_nep_171_arguments_and_decode_string_supplies() {
        let mock = Mock::default()
            .answer("nft_token", serde_json::Value::Null)
            .answer("nft_tokens_for_owner", json!([]))
            .answer("nft_total_supply", json!("12"))
            .answer("nft_supply_for_owner", json!("3"));
        let nft = NonFungibleToken::client(mock.clone(), account!("nft.near"));

        assert!(nft.token("token-1").await.unwrap().is_none());
        assert!(
            nft.tokens_for_owner(account!("alice.near"), Some(10), Some(5))
                .await
                .unwrap()
                .is_empty()
        );
        nft.tokens_for_owner(account!("alice.near"), None, None)
            .await
            .unwrap();
        assert_eq!(nft.total_supply().await.unwrap(), 12);
        assert_eq!(
            nft.supply_for_owner(account!("alice.near")).await.unwrap(),
            3
        );

        let seen = mock.seen.lock().unwrap().clone();
        assert_eq!(
            seen.into_iter().map(|(_, args)| args).collect::<Vec<_>>(),
            [
                json!({"token_id": "token-1"}),
                json!({"account_id": "alice.near", "from_index": "10", "limit": 5}),
                json!({"account_id": "alice.near"}),
                serde_json::Value::Null,
                json!({"account_id": "alice.near"}),
            ]
        );
    }

    #[tokio::test]
    async fn calls_preserve_nep_171_arguments() {
        let nft = NonFungibleToken::client(Mock::default(), account!("nft.near"));
        let tgas = Gas::from_tgas(1).as_gas();

        assert_eq!(
            sent_call(
                nft.transfer(account!("alice.near"), "token-1", None)
                    .await
                    .unwrap()
            ),
            (
                "nft_transfer".to_owned(),
                json!({"receiver_id": "alice.near", "token_id": "token-1"}),
                1,
                30 * tgas
            )
        );
        assert_eq!(
            sent_call(
                nft.transfer(account!("alice.near"), "token-1", Some("memo".into()))
                    .await
                    .unwrap()
            )
            .1,
            json!({"receiver_id": "alice.near", "token_id": "token-1", "memo": "memo"})
        );
        assert_eq!(
            sent_call(
                nft.transfer_with_approval(account!("alice.near"), "token-1", 7)
                    .await
                    .unwrap()
            )
            .1,
            json!({"receiver_id": "alice.near", "token_id": "token-1", "approval_id": 7})
        );
        assert_eq!(
            sent_call(
                nft.transfer_call(account!("market.near"), "token-1", "list")
                    .await
                    .unwrap()
            ),
            (
                "nft_transfer_call".to_owned(),
                json!({"receiver_id": "market.near", "token_id": "token-1", "msg": "list"}),
                1,
                100 * tgas
            )
        );
    }
}
