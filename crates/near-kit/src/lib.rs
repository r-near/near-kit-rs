//! A clean, ergonomic Rust client for NEAR Protocol.
//!
//! **near-kit** provides a fluent API for interacting with NEAR Protocol,
//! with a focus on developer experience and type safety.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use near_kit::*;
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
//!
//! # Typed Contract Interfaces
//!
//! Use the `#[near_kit::contract]` macro to create type-safe contract clients:
//!
//! ```ignore
//! use near_kit::*;
//! use serde::Serialize;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     fn get_count(&self) -> u64;
//!     
//!     #[call]
//!     fn increment(&mut self);
//! }
//!
//! let counter = near.contract::<Counter>("counter.testnet");
//! let count = counter.get_count().await?;
//! counter.increment().await?;
//! ```

pub mod client;
pub mod contract;
pub mod error;
pub mod tokens;
pub mod types;

// Sandbox module - only available with "sandbox" feature
#[cfg(feature = "sandbox")]
pub mod sandbox;

// Re-export commonly used types at crate root
pub use error::{Error, RpcError};
pub use types::nep413;
pub use types::*;

// Re-export contract types
pub use contract::{Contract, ContractClient};

// Re-export client types
pub use client::{
    AccessKeysQuery, AccountExistsQuery, AccountQuery, AddKeyCall, BalanceQuery, CallBuilder,
    ContractCall, DelegateOptions, DelegateResult, DeleteKeyCall, DeployCall, EnvSigner,
    FileSigner, InMemorySigner, Near, NearBuilder, Nep413SignFuture, RetryConfig, RotatingSigner,
    RpcClient, SandboxNetwork, SignFuture, Signer, TransactionBuilder, TransactionSend,
    TransferCall, ViewCall,
};

// Re-export token types
pub use tokens::{
    FtAmount, FtMetadata, FtTransferCall, FtTransferCallCall, FungibleToken, NftContractMetadata,
    NftToken, NftTokenMetadata, NftTransferCall, NftTransferCallCall, NonFungibleToken,
    StorageBalance, StorageBalanceBounds, StorageDepositCall,
};

// Re-export proc macros
pub use near_kit_macros::borsh;
pub use near_kit_macros::call;
pub use near_kit_macros::contract;
pub use near_kit_macros::json;
