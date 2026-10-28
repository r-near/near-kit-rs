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
//!     // `Err` means the transaction didn't run. A failed action is `Ok(outcome)`.
//!     if outcome.is_failure() {
//!         println!("transfer failed: {:?}", outcome.failure_message());
//!     }
//!     Ok(())
//! }
//! ```
//!
//! `Near::testnet()` and `Near::mainnet()` use FastNEAR's free, rate-limited
//! endpoints; `Near::custom(url, chain_id)` points at your own. See
//! [clients][guide-clients].
//!
//! ## Reading chain state
//!
//! `near.view(contract, method)` returns a [`View`]: set arguments with
//! `.args(..)`, pick a block with `.at(..)` (a height, a hash or a
//! [`Finality`](rpc::Finality)), then `.await` it for the decoded result.
//! Without `.at(..)` a read uses [`BlockReference::default()`](rpc::BlockReference),
//! the latest final block. `.with_metadata()` also returns the height and hash
//! the value was read at. `balance` and `account_exists` take the same
//! `.at(..)`, and every other RPC method is on [`near.rpc()`][rpc-client].
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::rpc::Finality;
//! use near_kit::*;
//!
//! # async fn example(near: &Near) -> Result<(), Error> {
//! let messages: Vec<serde_json::Value> = near
//!     .view(account!("guestbook.near-examples.testnet"), "get_messages")
//!     .args(serde_json::json!({ "limit": "5" }))
//!     .at(Finality::Optimistic)
//!     .await?;
//!
//! // A recent height: the default endpoints aren't archival.
//! let height = near.rpc().block(Finality::Final).await?.header.height;
//! let read = near
//!     .view::<u64>(account!("guestbook.near-examples.testnet"), "total_messages")
//!     .at(height)
//!     .with_metadata()
//!     .await?;
//! println!("{} at block {}", read.result, read.block_height);
//!
//! let exists = near.account_exists(account!("alice.testnet")).await?;
//! let keys = near
//!     .rpc()
//!     .view_access_key_list(&account!("alice.testnet"), Finality::Final)
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Sending transactions
//!
//! `alice.transaction(receiver)` starts a [`Tx`][tx]. Add
//! [`Action`](protocol::Action)s and [`FunctionCall`](transaction::FunctionCall)s
//! (30 Tgas and no deposit unless set) with `.add_action(..)`; `.await` signs,
//! sends and waits. The actions run in order, all or nothing. near-kit
//! fetches the nonce and block hash, re-signs with a fresh nonce when the
//! node rejects one (up to `max_nonce_retries`, 3 by default), and resends on
//! transient RPC errors.
//!
//! The result tells you whether the transaction ran:
//!
//! - `Ok(outcome)`: it executed. An action can still have failed (a contract
//!   panic, a missing account): check [`outcome.is_failure()`](rpc::FinalExecutionOutcome::is_failure),
//!   and read the return value with `outcome.result()` or `outcome.json()`.
//! - `Err(..)`: it didn't execute. [`Error::InvalidTx`] is a rejection by the
//!   node (bad nonce, not enough balance), [`Error::Validation`] a local check
//!   that failed before signing, and [`Error::Rpc`] a transport or RPC failure.
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::protocol::Action;
//! use near_kit::signer::SecretKey;
//! use near_kit::transaction::FunctionCall;
//! use near_kit::*;
//!
//! # async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
//! // One transaction: create, fund, add a key, deploy and initialise.
//! let key = SecretKey::generate_ed25519();
//! let wasm = std::fs::read("contract.wasm")?;
//! let outcome = alice
//!     .transaction(account!("app.alice.testnet"))
//!     .add_action(Action::create_account())
//!     .add_action(Action::transfer(NearToken::from_near(5)))
//!     .add_action(Action::add_full_access_key(key.public_key()))
//!     .add_action(Action::deploy_contract(wasm))
//!     .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner": alice.account_id() })))
//!     .await?;
//! if outcome.is_failure() {
//!     return Err(format!("deploy failed: {:?}", outcome.failure_message()).into());
//! }
//!
//! // A call with a deposit and more gas, decoding its return value.
//! let outcome = alice
//!     .transaction(account!("app.alice.testnet"))
//!     .add_action(
//!         FunctionCall::new("mint")
//!             .args(serde_json::json!({ "amount": "10" }))
//!             .deposit(NearToken::from_millinear(10))
//!             .gas(Gas::from_tgas(100)),
//!     )
//!     .await?;
//! let minted: u64 = outcome.json()?;
//! # Ok(())
//! # }
//! ```
//!
//! Sends wait for [`Final`](transaction::Final) execution. Pick another
//! [wait level](transaction::WaitLevel) once per handle with
//! `alice.wait_until::<Included>()`; see [finality][guide-finality].
//! `alice.sign(..)` signs without sending and `alice.sign_delegate(..)` builds a
//! NEP-366 meta-transaction; see [offline signing][guide-offline] and
//! [meta-transactions][guide-meta].
//!
//! ## Typed contracts
//!
//! With the `contracts` feature, `#[near_kit::contract]` turns a trait into a
//! client. Views need a [`Viewer`] and return a [`View`]; `#[call]` methods
//! need a [`TxSender`] and return a [`Call`] that takes `.gas(..)` and
//! `.deposit(..)`. The macro also generates offline constructors such as
//! `Counter::add(args)`, a [`FunctionCall`](transaction::FunctionCall) to batch
//! into a larger transaction.
//!
#![cfg_attr(all(feature = "rpc", feature = "contracts"), doc = "```rust,no_run")]
#![cfg_attr(
    not(all(feature = "rpc", feature = "contracts")),
    doc = "```rust,ignore"
)]
//! use near_kit::*;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     fn get_count(&self) -> u64;
//!
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//! }
//!
//! #[derive(serde::Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//!
//! # async fn example(near: &Near, alice: &SigningNear) -> Result<(), Error> {
//! let count = near.contract::<Counter>(account!("counter.testnet")).get_count().await?;
//!
//! let counter = alice.contract::<Counter>(account!("counter.testnet"));
//! counter.add(AddArgs { value: 5 }).gas(Gas::from_tgas(10)).await?.result()?;
//! # Ok(())
//! # }
//! ```
//!
//! The fungible and non-fungible token standards come as ready-made clients:
//! `near.contract::<FungibleToken>(id)` (see [`standards`]). The
//! [contracts and tokens][guide-contracts] chapter covers both.
//!
//! ## Guide
//!
//! | Chapter | Covers |
//! |---------|--------|
//! | [clients][guide-clients] | `Near` and `SigningNear`, networks and endpoints, `Near::custom`, the builder, one client per account |
//! | [transactions][guide-transactions] | `Tx` and every `Action`, batching typed calls, global contracts, state init, local checks |
//! | [finality][guide-finality] | Read finality, wait levels and what each returns, read-after-write, polling `tx_status` |
//! | [nonces][guide-nonces] | The nonce cache, `max_nonce_retries`, `RotatingSigner`, ordered queues, relayer settings |
//! | [errors][guide-errors] | `Error`, `ContractError`, `RpcError`; failed actions vs rejections; retries; your own error type |
//! | [meta_transactions][guide-meta] | NEP-366: signing a delegate action, sending it to a relayer, what a relayer checks |
//! | [offline_signing][guide-offline] | `sign` and `send_signed`, client-free transactions, base64, NEP-413 messages |
//! | [contracts_and_tokens][guide-contracts] | `#[contract]` in full, the FT and NFT clients |
//! | [testing][guide-testing] | Mock `Viewer` and `TxSender` backends, canned RPC transports, the sandbox |
//! | [keys][guide-keys] | Key types and formats, loading keys, seed phrases, implicit accounts, remote signers |
//! | [platforms][guide-platforms] | Browser and WASI builds, entropy backends, offline builds, contracts, tracing |
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
//! Items that need a feature say so in their docs. Building for a browser,
//! WASI or a contract is covered in [platforms][guide-platforms].
//!
#![cfg_attr(
    feature = "rpc",
    doc = "[rpc-client]: rpc::RpcClient
[tx]: transaction::Tx
[guide-clients]: crate::guide::clients
[guide-transactions]: crate::guide::transactions
[guide-finality]: crate::guide::finality
[guide-nonces]: crate::guide::nonces
[guide-errors]: crate::guide::errors
[guide-meta]: crate::guide::meta_transactions
[guide-offline]: crate::guide::offline_signing
[guide-testing]: crate::guide::testing
[guide-keys]: crate::guide::keys
[guide-platforms]: crate::guide::platforms"
)]
#![cfg_attr(
    not(feature = "rpc"),
    doc = "[rpc-client]: https://docs.rs/near-kit/latest/near_kit/rpc/struct.RpcClient.html
[tx]: https://docs.rs/near-kit/latest/near_kit/transaction/struct.Tx.html
[guide-clients]: https://docs.rs/near-kit/latest/near_kit/guide/clients/index.html
[guide-transactions]: https://docs.rs/near-kit/latest/near_kit/guide/transactions/index.html
[guide-finality]: https://docs.rs/near-kit/latest/near_kit/guide/finality/index.html
[guide-nonces]: https://docs.rs/near-kit/latest/near_kit/guide/nonces/index.html
[guide-errors]: https://docs.rs/near-kit/latest/near_kit/guide/errors/index.html
[guide-meta]: https://docs.rs/near-kit/latest/near_kit/guide/meta_transactions/index.html
[guide-offline]: https://docs.rs/near-kit/latest/near_kit/guide/offline_signing/index.html
[guide-testing]: https://docs.rs/near-kit/latest/near_kit/guide/testing/index.html
[guide-keys]: https://docs.rs/near-kit/latest/near_kit/guide/keys/index.html
[guide-platforms]: https://docs.rs/near-kit/latest/near_kit/guide/platforms/index.html"
)]
#![cfg_attr(
    all(feature = "rpc", feature = "contracts"),
    doc = "[guide-contracts]: crate::guide::contracts_and_tokens"
)]
#![cfg_attr(
    not(all(feature = "rpc", feature = "contracts")),
    doc = "[guide-contracts]: https://docs.rs/near-kit/latest/near_kit/guide/contracts_and_tokens/index.html"
)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Compiles the README's code blocks as doctests.
#[cfg(all(doctest, feature = "rpc"))]
#[doc = include_str!("../../../README.md")]
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
