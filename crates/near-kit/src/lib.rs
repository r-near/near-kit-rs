//! # near-kit
//!
//! A clean, ergonomic Rust client for NEAR Protocol.
//!
//! `near-kit` provides a fluent API for interacting with NEAR Protocol,
//! with a focus on developer experience and type safety. It's a ground-up
//! implementation with hand-rolled types based on actual NEAR RPC responses.
//!
//! ## Quick Start
//!
//! Add `near-kit` to your `Cargo.toml`:
//!
//! ```bash
//! cargo add near-kit
//! ```
//!
//! ### Read-Only Operations
//!
//! No credentials needed for querying blockchain state:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     let near = Near::testnet().build();
//!
//!     // Check account balance
//!     let balance = near.balance("alice.testnet").await?;
//!     println!("Balance: {}", balance.available);
//!
//!     // Call a view function
//!     let count: u64 = near.view("counter.testnet", "get_count").await?;
//!     println!("Count: {}", count);
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Transactions (Writes)
//!
//! For state-changing operations, configure a signer:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     let near = Near::testnet()
//!         .credentials("ed25519:YOUR_PRIVATE_KEY", "your-account.testnet")?
//!         .build();
//!
//!     // Transfer NEAR
//!     near.transfer("bob.testnet", NearToken::near(1)).await?;
//!
//!     // Call a contract function
//!     near.call("counter.testnet", "increment")
//!         .gas("30 Tgas")
//!         .await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Design Principles
//!
//! 1. **Single entry point** — Everything flows through the [`Near`] client
//! 2. **Configure once** — Network and signer are set at client creation
//! 3. **Type-safe but ergonomic** — Accept both typed values and string parsing
//! 4. **Explicit units** — No ambiguous amounts; must specify `NEAR`, `yocto`, `Tgas`, etc.
//! 5. **Progressive disclosure** — Simple things are simple; advanced options available when needed
//!
//! ## Core Types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`Near`] | Main client — the single entry point for all operations |
//! | [`AccountId`] | Validated NEAR account identifier |
//! | [`NearToken`] | Token amount with yoctoNEAR precision |
//! | [`Gas`] | Gas units for transactions |
//! | [`PublicKey`] / [`SecretKey`] | Ed25519 cryptographic keys |
//! | [`CryptoHash`] | 32-byte SHA-256 hash (blocks, transactions) |
//!
//! ## Working with Amounts
//!
//! ### Typed Constructors (Recommended)
//!
//! For compile-time safety, use the typed constructors:
//!
//! ```rust
//! use near_kit::{NearToken, Gas};
//!
//! // NEAR amounts
//! let five_near = NearToken::near(5);
//! let half_near = NearToken::millinear(500);
//! let one_yocto = NearToken::yocto(1);
//!
//! // Gas amounts
//! let gas = Gas::tgas(30);      // 30 teragas
//! let more_gas = Gas::ggas(5);  // 5 gigagas
//! ```
//!
//! ### String Parsing (Runtime Input)
//!
//! For CLI arguments, config files, or user input:
//!
//! ```rust
//! use near_kit::{NearToken, Gas};
//!
//! // Parse NEAR amounts
//! let amount: NearToken = "5 NEAR".parse().unwrap();
//! let small: NearToken = "100 milliNEAR".parse().unwrap();
//! let tiny: NearToken = "1000 yocto".parse().unwrap();
//!
//! // Parse gas
//! let gas: Gas = "30 Tgas".parse().unwrap();
//! ```
//!
//! Many builder methods also accept strings directly:
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # async fn example(near: &Near) -> Result<(), Error> {
//! near.call("contract.testnet", "method")
//!     .gas("100 Tgas")
//!     .deposit("0.1 NEAR")
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Query Builders
//!
//! Read operations return query builders that can be customized before awaiting:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! // Simple query
//! let balance = near.balance("alice.testnet").await?;
//!
//! // Query at a specific block height
//! let old_balance = near.balance("alice.testnet")
//!     .at_block(100_000_000)
//!     .await?;
//!
//! // Query with different finality
//! let optimistic = near.balance("alice.testnet")
//!     .finality(Finality::Optimistic)
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Transaction Builders
//!
//! Write operations return transaction builders for customization:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example(near: &Near) -> Result<(), Error> {
//! // Function call with arguments, gas, and deposit
//! near.call("nft.testnet", "nft_mint")
//!     .args(serde_json::json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
//!     .gas("100 Tgas")
//!     .deposit("0.1 NEAR")
//!     .await?;
//!
//! // Wait for different execution levels
//! near.transfer("bob.testnet", NearToken::near(1))
//!     .wait_until(TxExecutionStatus::Final)
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Multi-Action Transactions
//!
//! Chain multiple actions into a single atomic transaction:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example(near: &Near) -> Result<(), Box<dyn std::error::Error>> {
//! // Create a sub-account with funding and deploy a contract
//! let new_key: PublicKey = "ed25519:6E8sCci...".parse()?;
//! let wasm = std::fs::read("contract.wasm")?;
//!
//! near.transaction("sub.alice.testnet")
//!     .create_account()
//!     .transfer(NearToken::near(10))
//!     .add_full_access_key(new_key)
//!     .deploy(wasm)
//!     .call("new")
//!         .args(serde_json::json!({ "owner_id": "alice.testnet" }))
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Token Helpers
//!
//! Built-in support for fungible (NEP-141) and non-fungible (NEP-171) tokens.
//!
//! For common tokens like USDC, USDT, and wNEAR, use the provided [`tokens`] constants
//! to avoid copy-pasting addresses. They automatically resolve to the correct address
//! based on the network:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), Error> {
//! let near = Near::mainnet().build();
//!
//! // Known tokens auto-resolve based on network
//! let usdc = near.ft(tokens::USDC)?;
//! let balance = usdc.balance_of("alice.near").await?;
//! println!("Balance: {}", balance);  // "1.50 USDC"
//!
//! // Or use raw addresses for any token
//! let custom = near.ft("my-token.near")?;
//!
//! // NFTs (NEP-171)
//! let nft = near.nft("nft.testnet")?;
//! let tokens = nft.tokens_for_owner("alice.testnet", None, Some(10)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Available known tokens: [`tokens::USDC`], [`tokens::USDT`], [`tokens::W_NEAR`]
//!
//! ## Typed Contract Interfaces
//!
//! Use the `#[near_kit::contract]` macro for compile-time type safety:
//!
//! ```rust,ignore
//! use near_kit::*;
//! use serde::Serialize;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     // View method (read-only, no signer needed)
//!     fn get_count(&self) -> u64;
//!
//!     // Change method (requires signer)
//!     #[call]
//!     fn increment(&mut self);
//!
//!     // Change method with arguments
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//! }
//!
//! #[derive(Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//!
//! async fn example(near: &Near) -> Result<(), Error> {
//!     let counter = near.contract::<Counter>("counter.testnet");
//!
//!     // Type-safe view call
//!     let count = counter.get_count().await?;
//!
//!     // Type-safe change calls
//!     counter.increment().await?;
//!     counter.add(AddArgs { value: 5 }).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Signers
//!
//! Several signer implementations are available:
//!
//! | Signer | Use Case |
//! |--------|----------|
//! | [`InMemorySigner`] | Simple scripts with a private key |
//! | [`FileSigner`] | Load from `~/.near-credentials` (near-cli compatible) |
//! | [`EnvSigner`] | CI/CD environments via `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` |
//! | [`RotatingSigner`] | High-throughput with multiple keys (avoids nonce collisions) |
//! | [`KeyringSigner`] | System keyring (macOS Keychain, etc.) — requires `keyring` feature |
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # fn example() -> Result<(), Error> {
//! // Using credentials directly
//! let near = Near::testnet()
//!     .credentials("ed25519:...", "alice.testnet")?
//!     .build();
//!
//! // Using a custom signer
//! let signer = FileSigner::new("testnet", "alice.testnet")?;
//! let near = Near::testnet().signer(signer).build();
//!
//! // Environment variables (CI/CD)
//! let signer = EnvSigner::new()?;
//! let near = Near::testnet().signer(signer).build();
//! # Ok(())
//! # }
//! ```
//!
//! ### Multiple Accounts
//!
//! For production apps that manage multiple accounts, use [`Near::with_signer`] to
//! derive clients that share the same RPC connection but sign as different accounts:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
//! let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);
//!
//! // Both share the same connection, no overhead
//! // alice.transfer("carol.testnet", NearToken::near(1)).await?;
//! // bob.transfer("carol.testnet", NearToken::near(2)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Use [`with_signer`](Near::with_signer) for multi-account management and
//! [`RotatingSigner`] for high-throughput single-account usage (multiple keys to
//! avoid nonce collisions). For one-off overrides on a single transaction, use
//! [`.sign_with()`](TransactionBuilder::sign_with) on the transaction builder.
//!
//! ## Sandbox Testing
//!
//! Enable the `sandbox` feature for local testing with [`near-sandbox`](https://crates.io/crates/near-sandbox):
//!
//! ```toml
//! [dev-dependencies]
//! near-kit = { version = "0.1", features = ["sandbox"] }
//! near-sandbox = "0.3"
//! ```
//!
//! ```rust,ignore
//! use near_kit::*;
//! use near_sandbox::Sandbox;
//!
//! #[tokio::test]
//! async fn test_contract() {
//!     let sandbox = Sandbox::start().await.unwrap();
//!     let near = Near::sandbox(&sandbox);
//!
//!     // Root account is pre-configured with credentials
//!     near.transfer("alice.sandbox", NearToken::near(10)).await.unwrap();
//! }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `sandbox` | Integration with `near-sandbox` for local testing |
//! | `keyring` | System keyring signer (macOS Keychain, Windows Credential Manager, etc.) |
//!
//! ## Error Handling
//!
//! All operations return `Result<T, near_kit::Error>`. The [`Error`] type provides
//! detailed information about failures:
//!
//! ```rust,no_run
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! match near.balance("nonexistent.testnet").await {
//!     Ok(balance) => println!("Balance: {}", balance.available),
//!     Err(Error::Rpc(RpcError::AccountNotFound(account))) => {
//!         println!("Account {} doesn't exist", account);
//!     }
//!     Err(e) => return Err(e),
//! }
//! # Ok(())
//! # }
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
    AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, CallBuilder, DelegateOptions,
    DelegateResult, EnvSigner, FileSigner, InMemorySigner, Near, NearBuilder, RetryConfig,
    RotatingSigner, RpcClient, SandboxNetwork, Signer, SigningKey, TransactionBuilder,
    TransactionSend, ViewCall, ViewCallBorsh,
};

#[cfg(feature = "keyring")]
pub use client::KeyringSigner;

// Re-export token types
pub use tokens::{
    FtAmount, FtMetadata, FungibleToken, IntoContractId, KnownToken, NftContractMetadata, NftToken,
    NftTokenMetadata, NonFungibleToken, StorageBalance, StorageBalanceBounds, StorageDepositCall,
    USDC, USDT, W_NEAR,
};

// Re-export proc macros
pub use near_kit_macros::borsh;
pub use near_kit_macros::call;
pub use near_kit_macros::contract;
pub use near_kit_macros::json;
