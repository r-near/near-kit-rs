<div align="center">

# near-kit

**A clean, ergonomic Rust client for NEAR Protocol.**

[![Crates.io](https://img.shields.io/crates/v/near-kit.svg)](https://crates.io/crates/near-kit)
[![Documentation](https://docs.rs/near-kit/badge.svg)](https://docs.rs/near-kit)
[![CI](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/r-near/near-kit-rs/graph/badge.svg)](https://codecov.io/gh/r-near/near-kit-rs)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://github.com/r-near/near-kit-rs)

[API Docs](https://docs.rs/near-kit) · [Examples](crates/near-kit/examples/) · [Changelog](CHANGELOG.md)

</div>

---

## Why near-kit?

If you've worked with NEAR in Rust before, you've probably dealt with raw JSON-RPC calls, manual serialization, or incomplete client libraries. **near-kit** is designed to fix that.

It's a ground-up implementation focused on developer experience:

- **One entry point.** Everything flows through the `Near` client — no hunting for the right module.
- **Configure once.** Set your network and credentials at startup, then just write your logic.
- **Explicit units.** No more wondering if that's yoctoNEAR or NEAR. Write `NearToken::from_near(5)` or `"5 NEAR"`.
- **Batteries included.** Built-in FT/NFT helpers, multiple signer options,
  automatic retries, and opt-in typed-contract interfaces.

## Quick Start

```bash
cargo add near-kit
```

Reading from the blockchain doesn't require any credentials:

```rust
use near_kit::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let near = Near::testnet().build();

    let balance = near.balance("alice.testnet").await?;
    println!("Balance: {}", balance.available);

    let count: u64 = near.view("counter.testnet", "get_count").await?;
    println!("Count: {count}");

    Ok(())
}
```

## Sending Transactions

For writes, just add your credentials. The client handles nonce management, block references, signing, and retries automatically:

```rust
let near = Near::testnet()
    .credentials("ed25519:...", "alice.testnet")?
    .build();

// Transfer tokens
near.transfer("bob.testnet", NearToken::from_near(1)).await?;

// Call a contract function
near.call("counter.testnet", "increment")
    .gas(Gas::from_tgas(30))
    .await?;
```

For CI/CD, configure via environment variables:

```rust
// Reads NEAR_NETWORK, NEAR_ACCOUNT_ID, NEAR_PRIVATE_KEY
let near = Near::from_env()?;
```

Advanced APIs are grouped by purpose: `near_kit::rpc`, `near_kit::transaction`,
`near_kit::signer`, `near_kit::protocol`, and `near_kit::standards`. The crate
root keeps the main client, errors, account IDs, hashes, and gas/token units.

## Multiple Accounts

Transport and signing are separate concerns. Set up the connection once, then derive clients for different accounts with `with_signer`. They share the same RPC connection, so there's no overhead:

```rust
use near_kit::signer::InMemorySigner;

let near = Near::testnet().build(); // read-only, shared connection

let alice = near.with_signer(InMemorySigner::new("alice.testnet", "ed25519:...")?);
let bob = near.with_signer(InMemorySigner::new("bob.testnet", "ed25519:...")?);

alice.transfer("carol.testnet", NearToken::from_near(1)).await?;
bob.transfer("carol.testnet", NearToken::from_near(2)).await?;
```

For single-account scripts, the `credentials` builder is still the simplest path. Use `with_signer` when you need to manage multiple accounts or want explicit separation between connection setup and signing.

Need to pass arguments or attach a deposit? Chain the builders:

```rust
near.call("nft.testnet", "nft_mint")
    .args(serde_json::json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
    .deposit(NearToken::from_millinear(100))
    .gas(Gas::from_tgas(100))
    .await?;
```

## Multi-Action Transactions

NEAR supports batching multiple actions into a single atomic transaction. This is useful for creating accounts, deploying contracts, or any sequence that should succeed or fail together:

```rust
near.transaction("sub.alice.testnet")
    .create_account()
    .transfer(NearToken::from_near(5))
    .add_full_access_key(new_public_key)
    .deploy(wasm_bytes)
    .call("init")
        .args(serde_json::json!({ "owner": "alice.testnet" }))
    .send()
    .await?;
```

For more dynamic use cases, you can conditionally add actions or work with pre-built actions directly:

```rust
use near_kit::transaction::FunctionCall;

let mut tx = near.transaction("contract.testnet");

if needs_funding {
    tx = tx.transfer(NearToken::from_near(1));
}

tx = tx.call("setup")
    .args(serde_json::json!({ "admin": "alice.testnet" }))
    .finish(); // return to TransactionBuilder for more chaining

// Or add pre-built function calls
tx.add_action(FunctionCall::new("notify").args(serde_json::json!({ "msg": "hello" })))
    .send()
    .await?;
```

## Typed Contract Interfaces

Tired of stringly-typed method names and `serde_json::json!` everywhere? Define a trait for your contract and get compile-time checking:

Typed interfaces are opt-in so the default RPC client does not pull in proc-macro
dependencies:

```toml
[dependencies]
near-kit = { version = "0.18", features = ["contracts"] }
```

```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;          // view method

    #[call]
    fn increment(&mut self);              // change method
}

// Now you get autocomplete and type errors at compile time
let counter = near.contract::<Counter>("counter.testnet")?;
let count = counter.get_count().await?;
counter.increment().await?;
```

## Signers

Different situations call for different key management. near-kit supports several approaches:

| Signer | When to use it |
|--------|----------------|
| `signer::InMemorySigner` | Scripts and bots with a hardcoded or loaded key |
| `signer::FileSigner` | Local development — reads from `~/.near-credentials` (requires `file-signer`) |
| `signer::EnvSigner` | CI/CD pipelines via `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` |
| `signer::RotatingSigner` | High-throughput apps that need multiple keys to avoid nonce conflicts. Use `into_per_key_signers()` to split into per-key signers for sequential send queues |
| `signer::KeyringSigner` | Desktop apps using the system keychain (requires `keyring` feature) |

## Token Standards

Working with fungible or non-fungible tokens? near-kit includes helpers for NEP-141 and NEP-171.

For common tokens like USDC, USDT, and wNEAR, use the provided constants to avoid copy-pasting addresses. They automatically resolve to the correct address based on the network:

```rust
use near_kit::standards;

// Known tokens auto-resolve based on network
let near = Near::mainnet().build();
let usdc = near.ft(standards::USDC)?;
let balance = usdc.balance_of("alice.near").await?;
println!("Balance: {}", balance);  // "1.50 USDC"

// Or use raw addresses for any token
let custom = near.ft("my-custom-token.near")?;

// Non-fungible tokens
let nft = near.nft("nft.near")?;
if let Some(token) = nft.token("token-123").await? {
    println!("Owner: {}", token.owner_id);
}
```

Available known tokens: `standards::USDC`, `standards::USDT`, `standards::W_NEAR`

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rpc` | Yes | The `Near` client, queries, transactions, token helpers, and the HTTP transport — reqwest, except on WASI |
| `contracts` | No | Typed contract interfaces and macros; implies `rpc` |
| `wasi-http` | No | Built-in `wasi:http` transport for `wasm32-wasip2`; implies `rpc`, no-op elsewhere |
| `keyring` | No | System keyring integration for desktop apps |
| `file-signer` | No | Load signers from `~/.near-credentials` |
| `tracing` | No | [`tracing`](https://crates.io/crates/tracing) spans and events for RPC calls and transactions |
| `js` | No | JS-host entropy backend for `wasm32-unknown-unknown` |

### Tracing

With `tracing` on, RPC calls and transactions run inside spans (`call`, `view_function`, `send_transaction`, ...) and emit events at DEBUG (retries, failed requests, transaction lifecycle) and TRACE (raw request/response payloads). near-kit never logs at WARN or ERROR for an error it returns to you — that is the caller's decision — so a WARN-level subscriber stays quiet on expected failures such as probing a contract for a method it doesn't export. The one WARN is reserved for an anomaly that is *not* surfaced as an error: an RPC error variant this version couldn't parse and mapped to `Unknown`.

Docker-backed local testing lives in the companion `near-kit-sandbox` crate. Add
it as a dev-dependency and call `SandboxConfig::fresh().await?` or
`SandboxConfig::shared().await?`; each `Sandbox` can create a configured client
with `sandbox.client()` or `Near::sandbox(&sandbox)`.

### WASI (`wasm32-wasip2`)

near-kit runs inside WASI Preview 2 components with full RPC support — the `wasi-http` feature (implies `rpc`) provides a built-in `wasi:http/outgoing-handler` transport in place of reqwest:

```toml
[dependencies]
near-kit = { version = "0.14", default-features = false, features = ["wasi-http"] }
```

The host must provide the `wasi:http` interface (e.g. `wasmtime run -S http`, or any runtime targeting the `wasi:http/proxy` world). The transport is blocking — one request in flight at a time, the natural shape for a single-threaded component — and does not follow HTTP redirects, so point it at the final RPC URL.

On WASI hosts without `wasi:http`, enable only `rpc` and plug your platform's transport in via `NearBuilder::transport` — `wasi-http` must stay off there, because merely compiling the built-in transport makes the component import `wasi:http`, which such hosts refuse to instantiate. Or go fully offline (below).

### Offline / no-network usage

With `default-features = false` the RPC layer drops out and near-kit becomes a pure offline toolkit: protocol types under `near_kit::protocol`, signers under `near_kit::signer`, transaction construction and signing via `protocol::Transaction` (`new` → `sign` → `to_bytes`), and NEP-413 verification under `near_kit::standards::nep413`. This is what you want on targets without any network stack: sign transactions and messages locally and hand them off for submission elsewhere. Note the fluent `transaction::TransactionBuilder` belongs to the RPC layer (it is created from a `Near` client), so it requires the `rpc` feature.

### A note on `near-token` / `near-gas`

near-kit depends on and re-exports [`near-token`](https://crates.io/crates/near-token) and [`near-gas`](https://crates.io/crates/near-gas) — so `near_kit::NearToken` and `near_kit::Gas` *are* those crates' types. If you need an optional feature from either upstream crate, add `near-token` or `near-gas` as a direct dependency alongside near-kit with that feature enabled.

Use a version range compatible with near-kit's (check `cargo tree` if you're unsure). When only one version is resolved, Cargo unifies features across the graph and the re-exported types remain the same type — no conversions required. If you pin an incompatible semver range, Cargo will select two versions and the types will not be interchangeable.

## Documentation

For the full API reference, see [docs.rs/near-kit](https://docs.rs/near-kit).

## License

MIT
