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
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
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
//! For state-changing operations, attach a signer. That gives a
//! `SigningNear`, which sends transactions built from
//! [`Action`](protocol::Action)s:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::protocol::Action;
//! use near_kit::signer::InMemorySigner;
//! use near_kit::transaction::FunctionCall;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     let near = Near::testnet().build();
//!     let me = near.with_signer(InMemorySigner::new(
//!         "your-account.testnet",
//!         "ed25519:YOUR_PRIVATE_KEY",
//!     )?);
//!
//!     // Transfer NEAR
//!     me.transaction(account!("bob.testnet"))
//!         .add_action(Action::transfer(NearToken::from_near(1)))
//!         .await?;
//!
//!     // Call a contract function
//!     me.transaction(account!("counter.testnet"))
//!         .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(30)))
//!         .await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Design Principles
//!
//! 1. **Two clients** — `Near` reads; `SigningNear` (a `Near` plus a signer) also sends
//! 2. **Configure once** — Network, signer and wait level are set at client creation
//! 3. **Type-safe but ergonomic** — Accept both typed values and string parsing
//! 4. **Explicit units** — No ambiguous amounts; must specify `NEAR`, `yocto`, `Tgas`, etc.
//! 5. **Progressive disclosure** — Simple things are simple; advanced options available when needed
//!
//! ## Core Types
//!
//! | Type | Description |
//! |------|-------------|
//! | `Near` | Read-only client: chain reads and pre-signed submission |
//! | `SigningNear` | `Near` plus a signer: sends transactions |
//! | [`AccountId`] | Validated NEAR account identifier |
//! | [`NearToken`] | Token amount with yoctoNEAR precision |
//! | [`Gas`] | Gas units for transactions |
//! | [`signer::PublicKey`] / [`signer::SecretKey`] | Supported cryptographic keys |
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
//! let five_near = NearToken::from_near(5);
//! let half_near = NearToken::from_millinear(500);
//! let one_yocto = NearToken::from_yoctonear(1);
//!
//! // Gas amounts
//! let gas = Gas::from_tgas(30);      // 30 teragas
//! let more_gas = Gas::from_ggas(5);  // 5 gigagas
//! ```
//!
//! ### String Parsing (Runtime Input)
//!
//! For CLI arguments, config files, or user input:
//!
//! ```rust
//! use near_kit::{NearToken, Gas};
//!
//! // Parse NEAR amounts (upstream format)
//! let amount: NearToken = "5 NEAR".parse().unwrap();
//! let small: NearToken = "100 milliNEAR".parse().unwrap();
//! let tiny: NearToken = "1000 yoctoNEAR".parse().unwrap();
//!
//! // Parse gas
//! let gas: Gas = "30 Tgas".parse().unwrap();
//! ```
//!
//! ## Query Builders
//!
//! Read operations return query builders that can be customized before awaiting:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::rpc::Finality;
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
//!
//! // Contract code — regular accounts and the global registry
//! let contract = near.contract_code("app.near").await?;
//! let global = near.global_contract("publisher.near").await?;
//! println!("current global code hash: {}", global.hash);
//! if near.global_contract("publisher.near").exists().await? {
//!     println!("published!");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Transactions
//!
//! A transaction is a receiver plus a list of actions. Build the actions with
//! the [`Action`](protocol::Action) constructors (or typed
//! [`FunctionCall`](transaction::FunctionCall)s) and `.await`:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::protocol::Action;
//! use near_kit::signer::PublicKey;
//! use near_kit::transaction::{Final, FunctionCall};
//!
//! # async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
//! // Function call with arguments, gas, and deposit
//! alice
//!     .transaction(account!("nft.testnet"))
//!     .add_action(
//!         FunctionCall::new("nft_mint")
//!             .args(serde_json::json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
//!             .gas(Gas::from_tgas(100))
//!             .deposit(NearToken::from_millinear(100)),
//!     )
//!     .await?;
//!
//! // Create a sub-account with funding and deploy a contract, atomically,
//! // waiting for finality
//! let new_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
//! let wasm = std::fs::read("contract.wasm")?;
//! alice
//!     .wait_until::<Final>()
//!     .transaction(account!("sub.alice.testnet"))
//!     .add_action(Action::create_account())
//!     .add_action(Action::transfer(NearToken::from_near(10)))
//!     .add_action(Action::add_full_access_key(new_key))
//!     .add_action(Action::deploy_contract(wasm))
//!     .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner_id": "alice.testnet" })))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Token Helpers
//!
//! Built-in support for fungible (NEP-141) and non-fungible (NEP-171) tokens.
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//!
//! # async fn example(alice: &SigningNear) -> Result<(), Error> {
//! let token = alice.ft(account!("wrap.near"));
//! let balance = token.balance_of("alice.near").await?;
//! println!("Balance: {}", balance);
//!
//! // NFTs (NEP-171)
//! let nft = alice.nft(account!("nft.testnet"));
//! let tokens = nft.tokens_for_owner("alice.testnet", None, Some(10)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Typed Contract Interfaces
//!
//! Enable the opt-in `contracts` feature and use the `#[near_kit::contract]`
//! macro for compile-time type safety:
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
//! async fn example(near: &Near, alice: &SigningNear) -> Result<(), Error> {
//!     // Views work on any client
//!     let count = near.contract::<Counter>(account!("counter.testnet")).get_count().await?;
//!
//!     // Calls need a signing client
//!     let counter = alice.contract::<Counter>(account!("counter.testnet"));
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
//! | [`signer::InMemorySigner`] | Simple scripts with a private key |
//! | `signer::FileSigner` | Load from `~/.near-credentials` (near-cli compatible; requires `file-signer`) |
//! | [`signer::EnvSigner`] | CI/CD environments via `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` |
//! | [`signer::RotatingSigner`] | High-throughput with multiple keys (avoids nonce collisions) |
//! | `signer::KeyringSigner` | System keyring (macOS Keychain, etc.) — requires `keyring` feature |
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::signer::EnvSigner;
//!
//! # fn example() -> Result<(), Error> {
//! use near_kit::signer::InMemorySigner;
//!
//! let near = Near::testnet().build();
//!
//! // Using a key directly
//! let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
//!
//! // Environment variables (CI/CD)
//! let me = near.with_signer(EnvSigner::new()?);
//!
//! // Or network and credentials both from the environment
//! let me = SigningNear::from_env()?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Multiple Accounts
//!
//! For production apps that manage multiple accounts, use `Near::with_signer` to
//! derive signing clients that share the same RPC connection but sign as
//! different accounts:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//! use near_kit::signer::InMemorySigner;
//!
//! # fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
//! let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);
//!
//! // Both share the same connection, no overhead
//! assert_eq!(alice.near().rpc_url(), bob.near().rpc_url());
//! # Ok(())
//! # }
//! ```
//!
//! Use `Near::with_signer` for multi-account management and
//! [`signer::RotatingSigner`] for high-throughput single-account usage (multiple keys to
//! avoid nonce collisions).
//!
//! ## Sandbox Testing
//!
//! Use the companion `near-kit-sandbox` crate for Docker-backed local testing:
//!
//! ```toml
//! [dev-dependencies]
//! near-kit = "0.18"
//! near-kit-sandbox = "0.18"
//! ```
//!
//! ```rust,ignore
//! use near_kit::*;
//! use near_kit_sandbox::SandboxConfig;
//!
//! #[tokio::test]
//! async fn test_contract() -> Result<(), Box<dyn std::error::Error>> {
//!     let sandbox = SandboxConfig::fresh().await?;
//!     let root = sandbox.client(); // a SigningNear for the root account
//!
//!     root.transaction(account!("alice.sandbox"))
//!         .add_action(protocol::Action::transfer(NearToken::from_near(10)))
//!         .await?;
//!     Ok(())
//! }
//! ```
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
//! near-kit = { version = "0.18", default-features = false, features = ["js", "rpc"] }
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
//! near-kit = { version = "0.18", default-features = false, features = ["wasi-http"] }
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
//! near-kit = { version = "0.18", default-features = false }
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
//! The entire RPC layer — `Near`, `SigningNear`, the query builders, token helpers,
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
//! via [`transaction::FunctionCall`], and NEP-413
//! [`standards::nep413::verify_signature`].
//! Sending a `transaction::Tx` needs a `SigningNear`, so it requires `rpc`.
//! This is what you want on targets with no network stack at all — sign
//! transactions and messages locally, send them elsewhere:
//!
//! ```toml
//! [dependencies]
//! near-kit = { version = "0.18", default-features = false }
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
//!
//! ## Error Handling
//!
//! All operations return `Result<T, near_kit::Error>`. The [`Error`] type provides
//! detailed information about failures:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::*;
//!
//! # async fn example() -> Result<(), Error> {
//! let near = Near::testnet().build();
//!
//! match near.balance("nonexistent.testnet").await {
//!     Ok(balance) => println!("Balance: {}", balance.available),
//!     Err(e) => {
//!         println!("Error: {}", e);
//!         return Err(e);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

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
