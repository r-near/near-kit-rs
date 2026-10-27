//! Client module for interacting with NEAR Protocol.
//!
//! This module provides the core client infrastructure:
//!
//! - [`Near`] — The read-only client: chain reads and pre-signed submission
//! - [`SigningNear`] — A `Near` plus a signer: sends transactions
//! - [`NearBuilder`] — Fluent builder for configuring the client
//! - [`RpcClient`] — Low-level JSON-RPC client with retry logic
//! - [`RpcTransport`] — Pluggable HTTP layer under [`RpcClient`] (reqwest by
//!   default; `wasi:http` on `wasm32-wasip2`)
//!
//! # Signers
//!
//! Signers are used for transaction signing. Two implementations are
//! available, plus loaders that read a key once into an `InMemorySigner`:
//!
//! | Signer or loader | Use Case |
//! |--------|----------|
//! | [`InMemorySigner`](crate::signer::InMemorySigner) | One key in memory |
//! | [`RotatingSigner`](crate::signer::RotatingSigner) | High-throughput with multiple keys (avoids nonce collisions) |
//! | [`signer::from_env`](crate::signer::from_env) | CI/CD via `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` env vars |
//! | `signer::from_credentials` | `~/.near-credentials` (requires `file-signer`) |
//! | `signer::from_keyring` | The system keyring (requires `keyring`) |
//!
//! # Query Builders
//!
//! [`Near`] keeps the reads that add something to the matching
//! [`RpcClient`] method; each returns an awaitable value with one block
//! selector, `.at(..)`:
//!
//! - [`BalanceQuery`] — Get account balance
//! - [`AccountExistsQuery`] — Check if account exists
//! - [`TransactionStatusQuery`] — Poll or wait for transaction progress
//! - [`View`](crate::transaction::View) — Call view functions on contracts
//!
//! Every other read (accounts, access keys, contract code, validators, …)
//! is an [`RpcClient`] method, reached through [`Near::rpc`].
//!
//! # Transactions
//!
//! - [`Tx`] — A transaction from a [`SigningNear`]: add
//!   [`Action`](crate::protocol::Action)s, then `.await`
//! - [`FunctionCall`](crate::transaction::FunctionCall) — Standalone function call for composable transactions

// Everything that talks to the network lives behind the `rpc` feature; the
// signers stay available in offline builds (they only do local cryptography).
#[cfg(feature = "rpc")]
pub(crate) mod near;
#[cfg(feature = "rpc")]
mod nonce_manager;
#[cfg(feature = "rpc")]
mod query;
#[cfg(feature = "rpc")]
mod rpc;
mod signer;
#[cfg(feature = "rpc")]
mod signing;
#[cfg(feature = "rpc")]
mod transaction;
#[cfg(feature = "rpc")]
mod transport;

#[cfg(feature = "keyring")]
mod keyring;

#[cfg(feature = "rpc")]
pub use near::{Near, NearBuilder};
#[cfg(feature = "rpc")]
pub use query::{AccountExistsQuery, BalanceQuery, TransactionStatusQuery};
#[cfg(feature = "rpc")]
pub use rpc::{RetryConfig, RpcClient};
#[cfg(feature = "file-signer")]
pub use signer::{from_credentials, from_credentials_file};
pub use signer::{from_env, from_env_vars};
#[cfg(feature = "rpc")]
pub use signing::SigningNear;
#[cfg(feature = "rpc")]
pub use transaction::{DelegateOptions, SignedTransactionSend, Tx};
#[cfg(feature = "rpc")]
pub use transport::{BoxFuture, RpcTransport, TransportResponse};
// Only the operational built-in transport matching the build configuration is
// public: reqwest everywhere except WASI, and the wasi:http transport on
// wasm32-wasip2 with `wasi-http`. WASI without it uses a private erroring
// fallback until the caller injects a custom transport.
#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
pub use transport::ReqwestTransport;
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
pub use transport::WasiHttpTransport;

#[cfg(feature = "keyring")]
pub use keyring::from_keyring;
