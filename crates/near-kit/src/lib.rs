//! # near-kit
//!
//! A Rust client for NEAR Protocol, built from plain values and two verbs.
//!
//! Chain work is described by **values** that need no client: an
//! [`AccountId`] (write literals with [`account!`], checked at compile time),
//! [`NearToken`] and [`Gas`] amounts, [`Action`](protocol::Action)s and
//! [`FunctionCall`](transaction::FunctionCall)s, and a
//! [`BlockReference`](rpc::BlockReference) saying which block to read. Two
//! **verbs**, one small trait each, act on them:
//!
//! | Verb | Does | Implemented by |
//! |------|------|----------------|
//! | [`Viewer`] | runs a view function at a block | `Near`, `SigningNear` |
//! | [`TxSender`] | sends a receiver plus a list of actions | `SigningNear` |
//!
//! `Near` reads. `near.with_signer(signer)` returns a `SigningNear`, which
//! reads and sends; `alice.near()` gets the `Near` back. Typed contract
//! clients, the token clients and the awaitable [`View`] and [`Call`] values
//! are written against the two traits, so they run on your own backend too.
//!
//! The offline core (types, keys, signing, transaction building) is also
//! published alone as [`near-kit-primitives`](https://docs.rs/near-kit-primitives);
//! near-kit re-exports it at the same paths. Tests against a local node use
//! [`near-kit-sandbox`](https://docs.rs/near-kit-sandbox).
//!
//! ## Quick start
//!
//! ```sh
//! cargo add near-kit
//! cargo add tokio --features macros,rt-multi-thread
//! ```
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::protocol::Action;
//! use near_kit::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     // Reads need no signer and read the latest final block by default.
//!     let near = Near::testnet().build();
//!     let guestbook = account!("guestbook.near-examples.testnet");
//!     let balance = near.balance(&guestbook).await?;
//!     let messages: u64 = near.view(&guestbook, "total_messages").await?;
//!     println!("{} available, {messages} messages", balance.available);
//!
//!     // Sends need a signer. This one reads NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY.
//!     let alice = SigningNear::from_env()?;
//!     let outcome = alice
//!         .transaction(account!("bob.testnet"))
//!         .add_action(Action::transfer(NearToken::from_millinear(100)))
//!         .await?;
//!
//!     // A failed action is `Ok(outcome)`; `Err` means it didn't run, or that
//!     // near-kit couldn't tell whether it did (a timeout).
//!     if outcome.is_failure() {
//!         println!("transfer failed: {:?}", outcome.failure_message());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! `Near::testnet()` and `Near::mainnet()` use FastNEAR's free, rate-limited
//! endpoints; `Near::custom(url, chain_id)` points at your own.
//!
//! ## Guide
//!
//! The [guide] has a chapter per task: clients and endpoints, keys,
//! transactions, finality and wait levels, nonces, errors and retries,
//! meta-transactions, offline signing, typed contracts and tokens, testing,
//! and platforms.
//!
//! ## Feature flags
//!
//! | Feature | Default | Enables |
//! |---------|---------|---------|
//! | `rpc` | yes | `Near`, `SigningNear`, the token clients and the HTTP transport (reqwest, except on WASI). Without it near-kit is an offline toolkit: types, signers, transaction building, NEP-413 |
//! | `contracts` | no | `#[near_kit::contract]`. Without `rpc` it still generates the offline constructors and a client over any `Viewer` / `TxSender` |
//! | `file-signer` | no | `signer::from_credentials`: keys from `~/.near-credentials` |
//! | `keyring` | no | `signer::from_keyring`: keys from the system keyring |
//! | `mnemonic` | no | BIP-39 seed phrases and SLIP-10 key derivation |
//! | `tracing` | no | [`tracing`](https://docs.rs/tracing) spans and events for RPC calls and transactions |
//! | `wasi-http` | no | A built-in `wasi:http` transport for `wasm32-wasip2` (implies `rpc`) |
//! | `js` | no | Entropy from the JS host (browser, Node, Deno) on `wasm32-unknown-unknown` |
//!
//! Items that need a feature say so in their docs.
//!
#![cfg_attr(feature = "rpc", doc = "[guide]: crate::guide")]
#![cfg_attr(
    not(feature = "rpc"),
    doc = "[guide]: https://docs.rs/near-kit/latest/near_kit/guide/index.html"
)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Compiles the README's code blocks as doctests.
#[cfg(all(doctest, feature = "rpc"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

// Lets `#[near_kit::contract]` expansions name `::near_kit` even inside this
// crate.
#[cfg(feature = "contracts")]
extern crate self as near_kit;

mod client;
mod error;
#[cfg(feature = "rpc")]
mod platform;
#[cfg(feature = "rpc")]
mod tokens;
mod trace;
mod types;

pub mod guide;
pub mod protocol;
pub mod rpc;
pub mod signer;
pub mod standards;
pub mod transaction;

// The root is intentionally small; advanced APIs live in explicit facades.
pub use error::Error;
#[doc(inline)]
pub use near_kit_primitives::account;
#[doc(inline)]
pub use near_kit_primitives::{AccountId, AccountIdRef, CryptoHash, Gas, NearToken};
#[doc(inline)]
pub use near_kit_primitives::{
    Call, Contract, ContractError, MaybeSend, MaybeSync, TxSender, View, Viewer,
};

#[cfg(feature = "rpc")]
pub use client::{Near, NearBuilder, SigningNear};

/// The `reqwest` near-kit is built with, for the `reqwest::Client` that
/// [`ReqwestTransport::with_client`](crate::rpc::ReqwestTransport::with_client) takes.
#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
pub use reqwest;

// The typed-contract macro. Its `#[call]` / `#[json]` / `#[borsh]` markers are
// read by the macro itself and need no import.
#[cfg(feature = "contracts")]
pub use near_kit_macros::contract;

/// Support code for the `#[near_kit::contract]` expansion. Not public API.
///
/// Present without the `contracts` feature too: a crate that enables the
/// macro through near-kit-primitives but also depends on near-kit gets
/// expansions that name near-kit.
#[doc(hidden)]
pub mod __private {
    pub use near_kit_primitives::__private::{borsh_args, call, json_args, view_borsh, view_json};

    // A `cfg` in proc-macro output is evaluated against the *calling* crate's
    // features, not near-kit's. So the macro wraps its client struct in
    // `client_struct!`, and near-kit decides here, with its own features,
    // whether the backend defaults to `Near`.
    pub use crate::__near_kit_client_struct as client_struct;
}

#[doc(hidden)]
#[cfg(feature = "rpc")]
#[macro_export]
macro_rules! __near_kit_client_struct {
    ($(#[$attr:meta])* $vis:vis struct $name:ident<$backend:ident> $body:tt) => {
        $(#[$attr])* $vis struct $name<$backend = $crate::Near> $body
    };
}

#[doc(hidden)]
#[cfg(not(feature = "rpc"))]
#[macro_export]
macro_rules! __near_kit_client_struct {
    ($(#[$attr:meta])* $vis:vis struct $name:ident<$backend:ident> $body:tt) => {
        $(#[$attr])* $vis struct $name<$backend> $body
    };
}
