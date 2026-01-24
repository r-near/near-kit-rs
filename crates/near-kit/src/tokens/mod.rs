//! Token helpers for NEP-141 (Fungible Tokens) and NEP-171 (Non-Fungible Tokens).
//!
//! This module provides ergonomic APIs for interacting with standard NEAR token contracts.
//!
//! # Fungible Tokens (NEP-141)
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let near = Near::testnet().build();
//!
//! // Get a fungible token client
//! let usdc = near.ft("usdc.near")?;
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
//! let near = Near::testnet()
//!     .credentials("ed25519:...", "alice.near")?
//!     .build();
//! let usdc = near.ft("usdc.near")?;
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
//! ```

mod ft;
mod nft;
mod types;

pub use ft::*;
pub use nft::*;
pub use types::*;
