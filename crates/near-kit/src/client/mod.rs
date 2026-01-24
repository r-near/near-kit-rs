//! Client module for interacting with NEAR Protocol.
//!
//! This module provides the core client infrastructure:
//!
//! - [`Near`] — The main client, the single entry point for all operations
//! - [`NearBuilder`] — Fluent builder for configuring the client
//! - [`RpcClient`] — Low-level JSON-RPC client with retry logic
//!
//! # Signers
//!
//! Signers are used for transaction signing. Several implementations are available:
//!
//! | Signer | Use Case |
//! |--------|----------|
//! | [`InMemorySigner`] | Simple scripts with a private key in memory |
//! | [`FileSigner`] | Load from `~/.near-credentials` (near-cli compatible) |
//! | [`EnvSigner`] | CI/CD via `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` env vars |
//! | [`RotatingSigner`] | High-throughput with multiple keys (avoids nonce collisions) |
//!
//! # Query Builders
//!
//! Query builders provide a fluent API for read operations:
//!
//! - [`BalanceQuery`] — Get account balance
//! - [`AccountQuery`] — Get full account info
//! - [`AccountExistsQuery`] — Check if account exists
//! - [`AccessKeysQuery`] — List access keys
//! - [`ViewCall`] — Call view functions on contracts
//!
//! # Transaction Builders
//!
//! Transaction builders provide a fluent API for write operations:
//!
//! - [`TransactionBuilder`] — Multi-action transaction builder
//! - [`CallBuilder`] — Function call builder (part of transactions)

mod near;
mod nonce_manager;
mod query;
mod rpc;
mod signer;
mod transaction;

#[cfg(feature = "keyring")]
mod keyring_signer;

pub use near::{Near, NearBuilder, SandboxNetwork, SANDBOX_ROOT_ACCOUNT, SANDBOX_ROOT_PRIVATE_KEY};
pub use query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
pub use rpc::{RetryConfig, RpcClient};
pub use signer::{
    EnvSigner, FileSigner, InMemorySigner, Nep413SignFuture, RotatingSigner, SignFuture, Signer,
};
pub use transaction::{
    CallBuilder, DelegateOptions, DelegateResult, TransactionBuilder, TransactionSend,
};

#[cfg(feature = "keyring")]
pub use keyring_signer::KeyringSigner;
