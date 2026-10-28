<div align="center">

# near-kit

**A clean, ergonomic Rust client for NEAR Protocol.**

[![Crates.io](https://img.shields.io/crates/v/near-kit.svg)](https://crates.io/crates/near-kit)
[![Documentation](https://docs.rs/near-kit/badge.svg)](https://docs.rs/near-kit)
[![CI](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/r-near/near-kit-rs/graph/badge.svg)](https://codecov.io/gh/r-near/near-kit-rs)
[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue.svg)](https://github.com/r-near/near-kit-rs)

[API Docs](https://docs.rs/near-kit) · [Examples](crates/near-kit/examples/) · [Migration guide](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md) · [Changelog](https://github.com/r-near/near-kit-rs/blob/main/crates/near-kit/CHANGELOG.md)

</div>

---

## Why near-kit?

near-kit is a ground-up NEAR client with a small surface: chain work is
described by **plain values**, and two **verbs** (one trait each) do it:

- **Values** need no client: `AccountId` (write literals with `account!`, checked
  at compile time), `NearToken` and `Gas` (always with a unit), `Action` and
  `FunctionCall`, and a `BlockReference` for which block to read.
- **`Viewer`** runs a view function at a block. `Near` implements it.
- **`TxSender`** sends a receiver plus a list of actions.
  `near.with_signer(signer)` returns a `SigningNear`, which implements both.

Typed contract clients, the FT/NFT helpers and the awaitable `View`/`Call`
values are written against the two traits, so they also run on your own
backend.

## Quick Start

```bash
cargo add near-kit
```

```rust
use near_kit::protocol::Action;
use near_kit::signer::InMemorySigner;
use near_kit::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Reads need no credentials.
    let near = Near::testnet().build();
    let balance = near.balance(account!("alice.testnet")).await?;
    let count: u64 = near.view(account!("counter.testnet"), "get_count").await?;
    println!("{} available, count = {count}", balance.available);

    // Writes need a signer.
    let me = near.with_signer(InMemorySigner::new(account!("me.testnet"), "ed25519:...".parse()?));
    me.transaction(account!("bob.testnet"))
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await?;
    Ok(())
}
```

For CI, `SigningNear::from_env()` reads `NEAR_NETWORK`, `NEAR_ACCOUNT_ID` and
`NEAR_PRIVATE_KEY`.

## Reads

`near.view(..)` returns a `View`: add `.args(..)`, pick a block with `.at(..)`
(a height, a hash or a `Finality`; the default is `Final`), and `.await`.
`.with_metadata()` also gives the block height and hash. `balance` and
`account_exists` take the same `.at(..)`; `tx_status` picks how far the
transaction must have run with `.wait_until::<W>()`. Every other RPC method
is on `near.rpc()`.

```rust
use near_kit::rpc::Finality;

let messages: Vec<serde_json::Value> = near
    .view(account!("guestbook.testnet"), "get_messages")
    .args(serde_json::json!({ "limit": 5 }))
    .at(Finality::Optimistic)
    .await?;

let read = near
    .view::<u64>(account!("counter.testnet"), "get_count")
    .at(180_000_000)
    .with_metadata()
    .await?;
println!("{} at block {}", read.result, read.block_height);

let keys = near
    .rpc()
    .view_access_key_list(&account!("alice.testnet"), Finality::Final)
    .await?;
```

## Writes

A transaction is a receiver plus actions. `transaction(receiver)` collects
`Action`s and `FunctionCall`s and sends on `.await`, atomically. Nonces, block
hashes, signing and retries are handled for you.

```rust
use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;

me.transaction(account!("app.me.testnet"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(5)))
    .add_action(Action::add_full_access_key(new_key))
    .add_action(Action::deploy_contract(wasm))
    .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner_id": me.account_id() })))
    .await?;

me.transaction(account!("nft.testnet"))
    .add_action(
        FunctionCall::new("nft_mint")
            .args(serde_json::json!({ "token_id": "1", "receiver_id": "me.testnet" }))
            .deposit(NearToken::from_millinear(100))
            .gas(Gas::from_tgas(100)),
    )
    .await?;
```

Sends wait for `Final` execution by default. Choose another wait level once,
for every send from that handle:

```rust
use near_kit::transaction::Included;

let fast = me.wait_until::<Included>();
fast.transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?;
```

`me.sign(..)` signs without sending (submit later with `near.send_signed(..)`),
and `me.delegate(..)` builds NEP-366 meta-transactions.

## Multiple Accounts

Signing clients made from one `Near` share its RPC connection, so make one per
account:

```rust
let near = Near::testnet().build();
let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?));
let bob = near.with_signer(InMemorySigner::new(account!("bob.testnet"), "ed25519:...".parse()?));
```

`alice.near()` gets the read-only client back.

## Typed Contract Interfaces

Enable the opt-in `contracts` feature and describe the contract as a trait:

```toml
[dependencies]
near-kit = { version = "0.21", features = ["contracts"] }
```

```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;                // view

    #[call]
    fn add(&mut self, args: AddArgs);          // change
}

#[derive(serde::Serialize)]
pub struct AddArgs {
    pub value: u64,
}

// Views work on `Near`; calls need a `SigningNear`.
let count = near.contract::<Counter>(account!("counter.testnet")).get_count().await?;

let counter = me.contract::<Counter>(account!("counter.testnet"));
counter.add(AddArgs { value: 5 }).gas(Gas::from_tgas(10)).await?;
```

The macro also generates `Counter::add(args) -> FunctionCall`, for batching
with other actions.

## Token Standards

NEP-141 and NEP-171 clients. `FungibleToken` and `NonFungibleToken` in
`near_kit::standards` are `Contract` markers like the ones `#[contract]`
generates: `near.contract::<FungibleToken>(..)` reads, and
`me.contract::<FungibleToken>(..)` also transfers.

```rust
use near_kit::standards::{FungibleToken, NonFungibleToken};

let wrap = near.contract::<FungibleToken>(account!("wrap.near"));
let balance = wrap.balance_of(account!("alice.near")).await?;
println!("{balance}");

me.contract::<FungibleToken>(account!("wrap.near"))
    .transfer(account!("bob.near"), 1_000_u128, None)
    .await?;

let nft = near.contract::<NonFungibleToken>(account!("nft.near"));
if let Some(token) = nft.token("token-123").await? {
    println!("Owner: {}", token.owner_id);
}
```

## Your Own Backend

Implement `Viewer` or `TxSender` for a test double, a gateway or a send
queue, and typed contract clients, `View`/`Call` and the token helpers run on
it:

```rust
use near_kit::rpc::{BlockReference, ViewFunctionResult};

#[derive(Clone)]
struct Canned(&'static str);

impl Viewer for Canned {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        Ok(ViewFunctionResult::new(self.0.as_bytes().to_vec(), 1, CryptoHash::ZERO))
    }
}

let count = Counter::client(Canned("42"), account!("counter.near")).get_count().await?;
```

## Signers

| Signer or loader | When to use it |
|--------|----------------|
| `signer::InMemorySigner` | A key you hold in memory: `InMemorySigner::new(id, "ed25519:...".parse()?)` |
| `signer::RotatingSigner` | Several keys for one account, to avoid nonce conflicts. `into_per_key_signers()` splits it for sequential send queues |
| `signer::from_env()` | CI/CD: an `InMemorySigner` from `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` |
| `signer::from_credentials(network, id)` | Local development: reads `~/.near-credentials` (`file-signer` feature) |
| `signer::from_keyring(network, id, &public_key)` | Desktop apps using the system keychain (`keyring` feature) |

The loaders read the key once and return an `InMemorySigner`.

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rpc` | Yes | `Near`, `SigningNear`, the token helpers, and the HTTP transport — reqwest, except on WASI |
| `contracts` | No | Typed contract interfaces (`#[near_kit::contract]`). Without `rpc`, the generated client has no `Near` default and runs on any `Viewer` / `TxSender` |
| `wasi-http` | No | Built-in `wasi:http` transport for `wasm32-wasip2`; implies `rpc`, is a no-op on non-WASI targets, and is unsupported on earlier WASI targets |
| `keyring` | No | System keyring integration for desktop apps |
| `file-signer` | No | Load signers from `~/.near-credentials` |
| `tracing` | No | [`tracing`](https://crates.io/crates/tracing) spans and events for RPC calls and transactions |
| `mnemonic` | No | BIP-39 seed phrases and SLIP-10 hierarchical key derivation |
| `js` | No | JS-host entropy backend for `wasm32-unknown-unknown` |

### Tracing

With `tracing` on, RPC calls and transactions run inside spans (`call`, `view_function`, `send_transaction`, ...) and emit events at DEBUG (retries, failed requests, transaction lifecycle) and TRACE (raw request/response payloads). near-kit never logs at WARN or ERROR for an error it returns to you — that is the caller's decision — so a WARN-level subscriber stays quiet on expected failures such as probing a contract for a method it doesn't export. The one WARN is reserved for an anomaly that is *not* surfaced as an error: an RPC error variant this version couldn't parse and mapped to `Unknown`.

Docker-backed local testing lives in the companion `near-kit-sandbox` crate. Add
it as a dev-dependency and call `Sandbox::builder().fresh().await?` or
`Sandbox::builder().shared().await?`; `sandbox.client()` is a `SigningNear` for the
sandbox root account. See the
[`near-kit-sandbox` guide](https://github.com/r-near/near-kit-rs/blob/main/crates/near-kit-sandbox/README.md) for setup and
lifecycle details.

### WASI (`wasm32-wasip2`)

near-kit runs inside WASI Preview 2 components with full RPC support — the `wasi-http` feature (implies `rpc`) provides a built-in `wasi:http/outgoing-handler` transport in place of reqwest:

```toml
[dependencies]
near-kit = { version = "0.21", default-features = false, features = ["wasi-http"] }
```

The host must provide the `wasi:http` interface (e.g. `wasmtime run -S http`, or any runtime targeting the `wasi:http/proxy` world). The transport is blocking — one request in flight at a time, the natural shape for a single-threaded component — and does not follow HTTP redirects, so point it at the final RPC URL.

On WASI hosts without `wasi:http`, enable only `rpc` and plug your platform's
transport in via `NearBuilder::transport` — `wasi-http` must stay off there,
because merely compiling the built-in transport makes the component import
`wasi:http`, which such hosts refuse to instantiate. `NearBuilder::build()`
still succeeds if no custom transport is installed; the first RPC operation
returns a non-retryable `RpcError::Network` (wrapped in `Error::Rpc` by the
high-level client) instead of panicking. Or go fully offline (below).

### Offline / no-network usage

With `default-features = false` the RPC layer drops out and near-kit becomes a pure offline toolkit: protocol types under `near_kit::protocol`, signers under `near_kit::signer`, transaction construction and signing via `protocol::Transaction` (`new` → `sign` → `to_bytes`), and NEP-413 verification under `near_kit::standards::nep413`. The `Viewer` and `TxSender` traits are there too, so typed contract clients can run on a backend of your own. This is what you want on targets without any network stack: sign transactions and messages locally and hand them off for submission elsewhere.

### A note on `near-token` / `near-gas`

near-kit depends on and re-exports [`near-token`](https://crates.io/crates/near-token) and [`near-gas`](https://crates.io/crates/near-gas) — so `near_kit::NearToken` and `near_kit::Gas` *are* those crates' types. If you need an optional feature from either upstream crate, add `near-token` or `near-gas` as a direct dependency alongside near-kit with that feature enabled.

Use a version range compatible with near-kit's (check `cargo tree` if you're unsure). When only one version is resolved, Cargo unifies features across the graph and the re-exported types remain the same type — no conversions required. If you pin an incompatible semver range, Cargo will select two versions and the types will not be interchangeable.

## Documentation

For the full API reference, see [docs.rs/near-kit](https://docs.rs/near-kit).

## License

MIT
