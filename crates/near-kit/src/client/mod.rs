//! Client module for interacting with NEAR Protocol.
//!
//! This module provides the core client infrastructure:
//!
//! - [`Near`] — The main client, the single entry point for all operations
//! - [`NearBuilder`] — Fluent builder for configuring the client
//! - [`RpcClient`] — Low-level JSON-RPC client with retry logic
//! - [`RpcTransport`] — Pluggable HTTP layer under [`RpcClient`] (reqwest by
//!   default; `wasi:http` on `wasm32-wasip2`)
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
//! - [`TransactionStatusQuery`] — Poll or wait for transaction progress
//! - [`ViewCall`] — Call view functions on contracts
//!
//! # Transaction Builders
//!
//! Transaction builders provide a fluent API for write operations:
//!
//! - [`TransactionBuilder`] — Multi-action transaction builder
//! - [`CallBuilder`] — Function call builder (part of transactions)
//! - [`FunctionCall`] — Standalone function call for composable transactions

// Everything that talks to the network lives behind the `rpc` feature; the
// signers stay available in offline builds (they only do local cryptography).
#[cfg(feature = "rpc")]
mod near;
#[cfg(feature = "rpc")]
mod nonce_manager;
#[cfg(feature = "rpc")]
mod query;
#[cfg(feature = "rpc")]
mod rpc;
mod signer;
#[cfg(feature = "rpc")]
mod transaction;
#[cfg(feature = "rpc")]
mod transport;

#[cfg(feature = "keyring")]
mod keyring_signer;

#[cfg(feature = "rpc")]
pub use near::{Near, NearBuilder, SANDBOX_ROOT_ACCOUNT, SANDBOX_ROOT_SECRET_KEY, SandboxNetwork};
#[cfg(feature = "rpc")]
pub use query::{
    AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ContractCodeQuery,
    GlobalContractQuery, TransactionStatusQuery, ViewCall, ViewCallBorsh,
};
#[cfg(feature = "rpc")]
pub use rpc::{RetryConfig, RpcClient};
#[cfg(feature = "file-signer")]
pub use signer::FileSigner;
pub use signer::{EnvSigner, InMemorySigner, RotatingSigner, Signer, SigningKey};
#[cfg(feature = "rpc")]
pub use transaction::{
    CallBuilder, DelegateOptions, DelegateResult, FunctionCall, SignedTransactionSend,
    TransactionBuilder, TransactionSend,
};
#[cfg(feature = "rpc")]
pub use transport::{BoxFuture, RpcTransport, TransportResponse};
// Only the built-in transport matching the build target exists; the other side
// of the cfg pair would reference a dependency that isn't compiled in.
#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
pub use transport::ReqwestTransport;
#[cfg(all(feature = "rpc", target_arch = "wasm32", target_os = "wasi"))]
pub use transport::WasiHttpTransport;

#[cfg(feature = "keyring")]
pub use keyring_signer::KeyringSigner;
