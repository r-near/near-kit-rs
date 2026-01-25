//! Token helpers for NEP-141 (Fungible Tokens) and NEP-171 (Non-Fungible Tokens).
//!
//! This module provides ergonomic APIs for interacting with standard NEAR token contracts.
//!
//! # Known Token Constants
//!
//! For common tokens like USDC, USDT, and wNEAR, use the provided constants to avoid
//! copy-pasting addresses. These automatically resolve to the correct address based
//! on the network your client is connected to:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! // Mainnet client - USDC resolves to the mainnet address
//! let near = Near::mainnet().build();
//! let usdc = near.ft(tokens::USDC)?;
//! let balance = usdc.balance_of("alice.near").await?;
//!
//! // Testnet client - USDC resolves to the testnet address
//! let near = Near::testnet().build();
//! let usdc = near.ft(tokens::USDC)?;
//! # Ok(())
//! # }
//! ```
//!
//! Available tokens:
//!
//! | Constant | Token | Mainnet | Testnet |
//! |----------|-------|---------|---------|
//! | [`USDC`] | Circle USD Coin | ✓ | ✓ |
//! | [`USDT`] | Tether USD | ✓ | ✗ |
//! | [`W_NEAR`] | Wrapped NEAR | ✓ | ✓ |
//!
//! You can still use raw addresses for any token:
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # fn example(near: &Near) -> Result<(), near_kit::Error> {
//! let custom = near.ft("my-token.near")?;
//! # Ok(())
//! # }
//! ```
//!
//! # Fungible Tokens (NEP-141)
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let near = Near::mainnet().build();
//!
//! // Get a fungible token client using a known token
//! let usdc = near.ft(tokens::USDC)?;
//!
//! // Query metadata (cached after first call)
//! let metadata = usdc.metadata().await?;
//! println!("Token: {} ({})", metadata.name, metadata.symbol);
//!
//! // Query balance - returns FtAmount with decimals for formatting
//! let balance = usdc.balance_of("alice.near").await?;
//! println!("Balance: {}", balance);  // e.g., "1.5 USDC"
//!
//! // Transfer tokens (requires signer)
//! let near = Near::mainnet()
//!     .credentials("ed25519:...", "alice.near")?
//!     .build();
//! let usdc = near.ft(tokens::USDC)?;
//!
//! usdc.transfer("bob.near", 1_500_000_u128).await?;
//!
//! // Or with a memo
//! usdc.transfer_with_memo("bob.near", 1_500_000_u128, "Payment").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Non-Fungible Tokens (NEP-171)
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let near = Near::testnet().build();
//!
//! // Get an NFT client
//! let nft = near.nft("nft-contract.near")?;
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
//! // Transfer an NFT (requires signer)
//! let near = Near::testnet()
//!     .credentials("ed25519:...", "alice.near")?
//!     .build();
//! let nft = near.nft("nft-contract.near")?;
//!
//! nft.transfer("bob.near", "token-123").await?;
//!
//! // Or with a memo
//! nft.transfer_with_memo("bob.near", "token-123", "Gift").await?;
//! # Ok(())
//! # }
//! ```

mod ft;
mod known;
mod nft;
mod types;

pub use ft::*;
pub use known::{IntoContractId, KnownToken, USDC, USDT, W_NEAR};
pub use nft::*;
pub use types::*;
