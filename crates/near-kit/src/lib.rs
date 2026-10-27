//! # near-kit
//!
//! A Rust client for NEAR Protocol, built from plain values and two verbs.
//!
//! ## The model
//!
//! Chain work is described by **plain values** that need no client: an
//! [`AccountId`] (write literals with [`account!`], checked at compile time),
//! [`NearToken`] and [`Gas`] amounts, [`Action`](protocol::Action)s and
//! [`FunctionCall`](transaction::FunctionCall)s, and a
//! [`BlockReference`](rpc::BlockReference) saying which block to read.
//!
//! Two **verbs**, one small trait each, are what the rest is built on:
//!
//! | Verb | Does | Implemented by |
//! |------|------|----------------|
//! | [`Viewer`] | runs a view function at a block | `Near`, `SigningNear` |
//! | [`TxSender`] | sends a receiver plus a list of actions | `SigningNear` |
//!
//! `Near` reads. `near.with_signer(signer)` returns a `SigningNear`, which reads
//! and sends. Everything else (typed contract clients, the FT/NFT helpers, the
//! awaitable [`View`](transaction::View) and [`Call`](transaction::Call)
//! values) is written against the two traits, so it runs on your own backend
//! too.
//!
//! ## Quick start
//!
//! ```bash
//! cargo add near-kit
//! ```
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::protocol::Action;
//! use near_kit::signer::InMemorySigner;
//! use near_kit::transaction::FunctionCall;
//! use near_kit::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     // Reads need no credentials.
//!     let near = Near::testnet().build();
//!     let balance = near.balance(account!("alice.testnet")).await?;
//!     let count: u64 = near.view(account!("counter.testnet"), "get_count").await?;
//!     println!("{} available, count = {count}", balance.available);
//!
//!     // Writes need a signer.
//!     let me = near.with_signer(InMemorySigner::new(
//!         account!("me.testnet"),
//!         "ed25519:YOUR_PRIVATE_KEY".parse()?,
//!     ));
//!     me.transaction(account!("bob.testnet"))
//!         .add_action(Action::transfer(NearToken::from_near(1)))
//!         .await?;
//!     me.transaction(account!("counter.testnet"))
//!         .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(30)))
//!         .await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Reads
//!
//! `Near::view` returns a [`View`](transaction::View): set arguments with
//! `.args(..)`, pick a block with `.at(..)` (a height, a hash, or a
//! [`Finality`](rpc::Finality); the default is `Final`), then `.await`.
//! `.with_metadata()` also returns the block height and hash the value was
//! read at. `balance`, `account_exists` and `tx_status` take the same `.at(..)`.
//! Every other RPC method is on [`near.rpc()`](rpc::RpcClient).
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::rpc::{BlockReference, Finality};
//! use near_kit::*;
//!
//! # async fn example(near: &Near) -> Result<(), Error> {
//! let messages: Vec<serde_json::Value> = near
//!     .view(account!("guestbook.testnet"), "get_messages")
//!     .args(serde_json::json!({ "limit": 5 }))
//!     .at(Finality::Optimistic)
//!     .await?;
//!
//! let read = near
//!     .view::<u64>(account!("counter.testnet"), "get_count")
//!     .at(180_000_000)
//!     .with_metadata()
//!     .await?;
//! println!("{} at block {}", read.result, read.block_height);
//!
//! let old = near.balance(account!("alice.testnet")).at(180_000_000).await?;
//! let keys = near
//!     .rpc()
//!     .view_access_key_list(&account!("alice.testnet"), BlockReference::default())
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Writes
//!
//! `SigningNear::transaction(receiver)` collects actions with `.add_action(..)`
//! and sends on `.await`. Nonces, block hashes, signing and retries are
//! handled for you. Sends wait for `Final` execution by default; pick another
//! [wait level](transaction::WaitLevel) once with `.wait_until::<W>()`.
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::protocol::Action;
//! use near_kit::signer::SecretKey;
//! use near_kit::transaction::{FunctionCall, Included};
//! use near_kit::*;
//!
//! # async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
//! // One atomic transaction: create, fund, add a key, deploy and initialise.
//! let key = SecretKey::generate_ed25519();
//! let wasm = std::fs::read("contract.wasm")?;
//! alice
//!     .transaction(account!("app.alice.testnet"))
//!     .add_action(Action::create_account())
//!     .add_action(Action::transfer(NearToken::from_near(5)))
//!     .add_action(Action::add_full_access_key(key.public_key()))
//!     .add_action(Action::deploy_contract(wasm))
//!     .add_action(
//!         FunctionCall::new("new").args(serde_json::json!({ "owner_id": alice.account_id() })),
//!     )
//!     .await?;
//!
//! // Return as soon as the transaction is in a block.
//! let fast = alice.wait_until::<Included>();
//! fast.transaction(account!("bob.testnet"))
//!     .add_action(Action::transfer(NearToken::from_millinear(100)))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! `SigningNear::sign` signs without sending (submit later with
//! `Near::send_signed`) and `SigningNear::delegate` builds NEP-366
//! meta-transactions.
//!
//! ## Typed contracts
//!
//! With the opt-in `contracts` feature, `#[near_kit::contract]` turns a trait
//! into a client. Views need a [`Viewer`] and return a `View`; `#[call]` methods
//! need a [`TxSender`] and return a [`Call`](transaction::Call) that takes
//! `.gas(..)` and `.deposit(..)`.
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
//! counter.add(AddArgs { value: 5 }).gas(Gas::from_tgas(10)).await?;
//! let count = counter.get_count().at(180_000_000).await?;
//! # Ok(())
//! # }
//! ```
//!
//! The macro also generates offline constructors such as
//! `Counter::add(args) -> FunctionCall`, for adding to a multi-action
//! transaction.
//!
//! ## Tokens
//!
//! NEP-141 and NEP-171 helpers: `near.ft(id)` / `near.nft(id)` read, and
//! `signing.ft(id)` / `signing.nft(id)` also transfer.
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//!
//! # async fn example(near: &Near, alice: &SigningNear) -> Result<(), Error> {
//! let balance = near.ft(account!("wrap.near")).balance_of(account!("alice.near")).await?;
//! println!("{balance}");
//!
//! alice
//!     .ft(account!("wrap.near"))
//!     .transfer(account!("bob.near"), 1_000_u128, None)
//!     .await?;
//!
//! let nfts = near
//!     .nft(account!("nft.near"))
//!     .tokens_for_owner(account!("alice.near"), None, Some(10))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Your own backend
//!
//! Implement [`Viewer`] or [`TxSender`] and the typed clients, `View`/`Call`
//! and the token helpers run on it: a test double, a gateway, or a sender
//! that queues actions elsewhere. Both traits are also implemented for `&T`,
//! `Box<T>` and `Arc<T>`.
//!
//! ```rust
//! use near_kit::rpc::{BlockReference, ViewFunctionResult};
//! use near_kit::transaction::View;
//! use near_kit::*;
//!
//! /// Answers every view with the same JSON.
//! struct Canned(&'static str);
//!
//! impl Viewer for Canned {
//!     type ViewError = String;
//!
//!     async fn view_function(
//!         &self,
//!         _contract_id: &AccountId,
//!         _method: &str,
//!         _args: &[u8],
//!         _block: BlockReference,
//!     ) -> Result<ViewFunctionResult, String> {
//!         Ok(ViewFunctionResult::new(self.0.as_bytes().to_vec(), 1, CryptoHash::ZERO))
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let count: u64 = View::json(Canned("42"), account!("counter.near"), "get_count").await?;
//! assert_eq!(count, 42);
//! # Ok(())
//! # }
//! ```
//!
//! ## Signers
//!
//! | Signer | Use case |
//! |--------|----------|
//! | [`signer::InMemorySigner`] | A key you hold in memory |
//! | [`signer::EnvSigner`] | `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` (CI) |
//! | [`signer::RotatingSigner`] | Several keys for one account, to avoid nonce collisions |
//! | `signer::FileSigner` | `~/.near-credentials` (near-cli compatible; `file-signer` feature) |
//! | `signer::KeyringSigner` | The system keyring (`keyring` feature) |
//!
//! Signing clients made from one `Near` share its connection, so one client
//! per account is cheap. `SigningNear::from_env()` reads the network and the
//! credentials from the environment.
//!
//! ## Amounts
//!
//! Amounts always carry a unit: `NearToken::from_near(5)`,
//! `NearToken::from_millinear(500)`, `Gas::from_tgas(30)`. Runtime input
//! parses with `FromStr`:
//!
//! ```rust
//! use near_kit::{Gas, NearToken};
//!
//! let amount: NearToken = "5 NEAR".parse()?;
//! let gas: Gas = "30 Tgas".parse()?;
//! assert_eq!(amount, NearToken::from_near(5));
//! assert_eq!(gas, Gas::from_tgas(30));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Errors
//!
//! Queries such as `balance` and `transaction(..)` sends return
//! [`Error`]. A `View` or `Call` (including typed-contract and
//! token methods) returns [`ContractError<E>`](transaction::ContractError):
//! a codec error for the arguments or result, or the backend's own error `E`
//! (`RpcError` for `Near`'s views). `?` converts it into `Error`.
//!
//! ## Sandbox testing
//!
//! The companion [`near-kit-sandbox`](https://docs.rs/near-kit-sandbox) crate
//! starts a Docker-backed local node; `sandbox.client()` is a `SigningNear` for
//! its root account.
//!
//! ## WebAssembly support
//!
//! `near-kit` compiles for `wasm32-unknown-unknown` (Dioxus, Leptos, Yew, etc.) with
//! `default-features = false`. The `keyring` and `file-signer` features use OS APIs
//! unavailable in the browser and should stay off; use [`signer::InMemorySigner`] or
//! [`signer::EnvSigner`] instead. Add the opt-in `tracing` feature if you want spans
//! (the `tracing` crate itself is wasm-compatible). Add `rpc` to keep the `Near`
//! client, which speaks HTTP through the browser's `fetch` on this target.
//!
//! `near-kit` relies directly on `getrandom` for key generation and nonce randomness.
//! On `wasm32-unknown-unknown`, `getrandom` requires the embedder to pick an entropy
//! backend. If your wasm target runs in a
//! browser (or another JS host like Node or Deno), enable the `js` feature to use the
//! host's `crypto.getRandomValues`:
//!
//! ```toml
//! [dependencies]
//! near-kit = { version = "0.21", default-features = false, features = ["js", "rpc"] }
//! ```
//!
//! If you're running `wasm32-unknown-unknown` outside a JS host, leave `js` off and
//! register your own `getrandom` backend instead (see below) — forcing the browser
//! backend would break your runtime. Leave `rpc` off there too: the HTTP client is
//! `fetch`-backed on this target, so without a JS host only the offline core (see
//! below) works. (WASI targets like `wasm32-wasip1` don't need any of this:
//! `getrandom` supports them natively, so leave `js` off there too.)
//!
//! ### WASI (`wasm32-wasip2`)
//!
//! near-kit also runs inside WASI Preview 2 components, with full RPC support:
//! the opt-in `wasi-http` feature (which implies `rpc`) provides a built-in
//! `WasiHttpTransport` speaking `wasi:http/outgoing-handler` in place of
//! reqwest, which doesn't build for WASI.
//!
//! ```toml
//! [dependencies]
//! near-kit = { version = "0.21", default-features = false, features = ["wasi-http"] }
//! ```
//!
//! The host must provide the `wasi:http` interface — `wasmtime run -S http`, or
//! any runtime targeting the `wasi:http/proxy` world. Three semantic
//! differences from native:
//!
//! - The transport is *blocking*: the guest parks until each response arrives,
//!   so one request is in flight at a time and concurrent RPC calls serialize.
//! - Retry backoff uses `std::thread::sleep` (no async runtime in the guest).
//! - HTTP redirects are not followed (reqwest follows up to 10 by default) —
//!   point the client at the final RPC URL.
//!
//! On WASI hosts *without* `wasi:http`, enable only `rpc` and plug in whatever
//! the platform does provide via `NearBuilder::transport`. The `wasi-http`
//! feature must stay off there: merely compiling the built-in transport makes
//! the component import `wasi:http`, which such hosts refuse to instantiate.
//! With `rpc` alone those imports never appear. A client can still be built,
//! but its RPC calls return a non-retryable [`rpc::RpcError::Network`] until a
//! transport is injected. (Or stay fully offline by dropping `rpc` too — see
//! below.)
//!
//! ### Custom entropy backends
//!
//! Non-JS `wasm32-unknown-unknown` embedders must register one entropy backend, for
//! getrandom 0.4 — the only major version reachable on that target, which `rand`,
//! `k256`, `ed25519-dalek`, and `ml-dsa` all share through `rand_core` 0.10. (A
//! native `cargo tree` also shows getrandom 0.2 behind `rpc`, via
//! `ring`/`rustls`; that native TLS stack does not build for
//! `wasm32-unknown-unknown`, so it does not need a backend here.)
//!
//! ```toml
//! [dependencies]
//! near-kit = { version = "0.21", default-features = false }
//! ```
//!
//! Build with `RUSTFLAGS='--cfg getrandom_backend="custom"'` and export an `extern
//! "Rust"` fn named `__getrandom_v03_custom` returning `Result<(), getrandom::Error>`
//! — note the `v03`: getrandom 0.4 kept the 0.3 symbol name, so `_v04_` won't link.
//! See the [`getrandom` docs](https://docs.rs/getrandom) for the exact signature. If
//! your application never generates keys or nonces,
//! `--cfg getrandom_backend="unsupported"` also works: it compiles everywhere and
//! errors at runtime if entropy is ever requested.
//!
//! <div class="warning">
//!
//! **Changed since 0.14:** near-kit used to pull in *two* getrandom majors (0.2 and
//! 0.4) and needed a backend for each — the 0.2 one registered with
//! `register_custom_getrandom!` behind its `custom` feature. Unifying the crypto
//! graph on `rand_core` 0.10 dropped getrandom 0.2 entirely, so that half of the
//! setup (and the direct `getrandom = "0.2"` dependency it required) should now be
//! removed.
//!
//! </div>
//!
//! ## Offline / no-network usage
//!
//! The entire RPC layer — `Near`, `SigningNear`, the token helpers,
//! and the HTTP client underneath — is gated behind the default-on `rpc` feature.
//! Typed-contract interfaces and macros additionally require `contracts`; with
//! `contracts` but not `rpc`, `#[near_kit::contract]` still generates the
//! offline `FunctionCall` constructors (e.g. `Counter::increment()`) and the
//! backend-generic `CounterClient<C>`, whose backend then has no `Near`
//! default. With
//! `default-features = false` you keep the offline core:
//! all the types, the signers ([`signer::InMemorySigner`],
//! [`signer::EnvSigner`], ...), transaction construction and signing via
//! [`protocol::Transaction`] (`new` → `sign` → `to_bytes`), standalone call construction
//! via [`transaction::FunctionCall`], NEP-413
//! [`standards::nep413::verify_signature`], and the [`Viewer`] / [`TxSender`]
//! traits for plugging in a backend of your own.
//! This is what you want on targets with no network stack at all — sign
//! transactions and messages locally, send them elsewhere:
//!
//! ```toml
//! [dependencies]
//! near-kit = { version = "0.21", default-features = false }
//! ```
//!
//! The offline core is also published on its own as
//! [`near-kit-primitives`](https://docs.rs/near-kit-primitives), which never
//! depends on an async runtime or HTTP client. near-kit re-exports it at the
//! paths above, so its types are near-kit's types.
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `rpc` | Yes | The RPC layer: `Near`, `SigningNear`, queries, transactions, tokens, and the HTTP transport (reqwest, except on WASI). Disable for offline signing/verification |
//! | `contracts` | No | Typed contract interfaces and the `#[near_kit::contract]` macro. Works without `rpc` (offline `FunctionCall` constructors and a client over any `Viewer`/`TxSender`); `Near::contract` needs `rpc` |
//! | `wasi-http` | No | Built-in `wasi:http` transport for `wasm32-wasip2` (implies `rpc`; no-op on non-WASI targets; unsupported on earlier WASI targets). On WASI Preview 2 hosts without `wasi:http`, inject a transport via `NearBuilder::transport` |
//! | `keyring` | No | System keyring signer (macOS Keychain, Windows Credential Manager, etc.) |
//! | `file-signer` | No | `signer::FileSigner` for loading keys from `~/.near-credentials` |
//! | `tracing` | No | [`tracing`](https://docs.rs/tracing) spans and events for RPC calls and transactions (see below) |
//! | `mnemonic` | No | BIP-39 seed phrases and SLIP-10 hierarchical key derivation |
//! | `js` | No | JS-host entropy backend (`getrandom`'s `wasm_js`) for `wasm32-unknown-unknown` |
//!
//! ### Tracing
//!
//! With `tracing` on, RPC calls and transactions run inside spans (`call`,
//! `view_function`, `send_transaction`, ...) and emit events at DEBUG (retries,
//! failed requests, transaction lifecycle) and TRACE (raw request/response
//! payloads). near-kit never logs at WARN or ERROR for an error it returns to you —
//! that is the caller's decision — so a WARN-level subscriber stays quiet on
//! expected failures such as probing a contract for a method it doesn't export.
//! The one WARN is reserved for an anomaly that is *not* surfaced as an error:
//! an RPC error variant this version couldn't parse and mapped to `Unknown`.

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
pub use near_kit_primitives::{AccountId, CryptoHash, Gas, NearToken};
#[doc(inline)]
pub use near_kit_primitives::{Contract, MaybeSend, MaybeSync, TxSender, Viewer};

#[cfg(feature = "rpc")]
pub use client::{Near, NearBuilder, SigningNear};

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
