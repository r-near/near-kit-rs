//! Token helpers for NEP-141 (Fungible Tokens) and NEP-171 (Non-Fungible Tokens).
//!
//! This module provides ergonomic APIs for interacting with standard NEAR token contracts.
//!
//! Create a token client from a [`SigningNear`](crate::SigningNear) and the
//! contract's account ID. It reads through the client's
//! [`Near`](crate::Near) and sends its calls as the client's account:
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # fn example(alice: &SigningNear) -> Result<(), near_kit::Error> {
//! let token = alice.ft(account!("wrap.near"));
//! # Ok(())
//! # }
//! ```
//!
//! # Fungible Tokens (NEP-141)
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
//! let token = alice.ft(account!("wrap.near"));
//!
//! // Query metadata (cached after first call)
//! let metadata = token.metadata().await?;
//! println!("Token: {} ({})", metadata.name, metadata.symbol);
//!
//! // Query balance - returns FtAmount with decimals for formatting
//! let balance = token.balance_of("alice.near").await?;
//! println!("Balance: {}", balance);
//!
//! // Transfer tokens as alice
//! token.transfer("bob.near", 1_500_000_u128).await?;
//!
//! // Or with a memo
//! token.transfer_with_memo("bob.near", 1_500_000_u128, "Payment").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Non-Fungible Tokens (NEP-171)
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example(alice: SigningNear) -> Result<(), near_kit::Error> {
//! let nft = alice.nft(account!("nft-contract.near"));
//!
//! // Query a specific token
//! if let Some(token) = nft.token("token-123").await? {
//!     println!("Owner: {}", token.owner_id);
//! }
//!
//! // List tokens owned by an account
//! let tokens = nft.tokens_for_owner("alice.near", None, Some(10)).await?;
//! for token in tokens {
//!     println!("Token: {}", token.token_id);
//! }
//!
//! // Transfer an NFT as alice
//! nft.transfer("bob.near", "token-123").await?;
//!
//! // Or with a memo
//! nft.transfer_with_memo("bob.near", "token-123", "Gift").await?;
//! # Ok(())
//! # }
//! ```

mod ft;
mod nft;
mod types;

pub use ft::*;
pub use nft::*;
pub use types::*;
