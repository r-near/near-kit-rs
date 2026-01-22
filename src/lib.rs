//! A clean, ergonomic Rust client for NEAR Protocol.
//!
//! **near-kit** provides a fluent API for interacting with NEAR Protocol,
//! with a focus on developer experience and type safety.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use near_kit::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), near_kit::Error> {
//!     // Configure once
//!     let near = Near::testnet().build();
//!     
//!     // Check balance
//!     let balance = near.balance("alice.testnet").await?;
//!     println!("Balance: {}", balance);
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Design Principles
//!
//! 1. **Single entry point**: Everything hangs off the [`Near`] client
//! 2. **Configure once**: Network and signer set at client creation
//! 3. **Type-safe but ergonomic**: Accept `"5 NEAR"` strings while maintaining type safety
//! 4. **Explicit units**: No ambiguous amounts - must specify `NEAR`, `yocto`, `Tgas`, etc.
//! 5. **Progressive disclosure**: Simple things are simple, advanced options available when needed
//!
//! # Core Types
//!
//! - [`AccountId`] - Validated NEAR account identifier
//! - [`NearToken`] - NEAR token amount with yoctoNEAR precision
//! - [`Gas`] - Gas units for transactions  
//! - [`PublicKey`], [`SecretKey`] - Cryptographic keys
//! - [`CryptoHash`] - 32-byte SHA-256 hash
//!
//! # String Parsing
//!
//! Many types support parsing from human-readable strings:
//!
//! ```
//! use near_kit::{NearToken, Gas, AccountId};
//!
//! let amount: NearToken = "5 NEAR".parse().unwrap();
//! let gas: Gas = "30 Tgas".parse().unwrap();
//! let account: AccountId = "alice.testnet".parse().unwrap();
//! ```

pub mod client;
pub mod error;
pub mod types;

// Re-export commonly used types at crate root
pub use error::{Error, RpcError};
pub use types::*;

// Re-export client types
pub use client::{
    AccessKeysQuery, AccountExistsQuery, AccountQuery, AddKeyCall, BalanceQuery, CallBuilder,
    ContractCall, DeleteKeyCall, DeployCall, InMemoryKeyStore, KeyStore, KeyStoreSigner, Near,
    NearBuilder, RetryConfig, RpcClient, Signer, TransactionBuilder, TransactionSend, TransferCall,
    ViewCall,
};

/// Prelude module for convenient imports.
///
/// ```
/// use near_kit::prelude::*;
/// ```
pub mod prelude {
    pub use crate::client::{
        InMemoryKeyStore, KeyStore, KeyStoreSigner, Near, NearBuilder, Signer,
    };
    pub use crate::types::{
        AccountId, BlockReference, CryptoHash, Finality, Gas, NearToken, PublicKey, SecretKey,
        TxExecutionStatus,
    };
    pub use crate::Error;
}
