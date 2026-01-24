<div align="center">

# near-kit

**A clean, ergonomic Rust client for NEAR Protocol.**

[![Crates.io](https://img.shields.io/crates/v/near-kit.svg)](https://crates.io/crates/near-kit)
[![Documentation](https://docs.rs/near-kit/badge.svg)](https://docs.rs/near-kit)
[![CI](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/r-near/near-kit-rs/graph/badge.svg)](https://codecov.io/gh/r-near/near-kit-rs)
[![MSRV](https://img.shields.io/badge/MSRV-1.86-blue.svg)](https://github.com/r-near/near-kit-rs)

[API Docs](https://docs.rs/near-kit) · [Examples](crates/near-kit/examples/) · [Changelog](CHANGELOG.md)

</div>

---

## Why near-kit?

If you've worked with NEAR in Rust before, you've probably dealt with raw JSON-RPC calls, manual serialization, or incomplete client libraries. **near-kit** is designed to fix that.

It's a ground-up implementation focused on developer experience:

- **One entry point.** Everything flows through the `Near` client — no hunting for the right module.
- **Configure once.** Set your network and credentials at startup, then just write your logic.
- **Explicit units.** No more wondering if that's yoctoNEAR or NEAR. Write `NearToken::near(5)` or `"5 NEAR"`.
- **Batteries included.** Built-in support for FT/NFT standards, typed contracts, multiple signers, and automatic retries.

## Quick Start

```toml
[dependencies]
near-kit = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
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
near.transfer("bob.testnet", NearToken::near(1)).await?;

// Call a contract function
near.call("counter.testnet", "increment")
    .gas(Gas::tgas(30))
    .await?;
```

Need to pass arguments or attach a deposit? Chain the builders:

```rust
near.call("nft.testnet", "nft_mint")
    .args(serde_json::json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
    .deposit(NearToken::millinear(100))
    .gas(Gas::tgas(100))
    .await?;
```

## Multi-Action Transactions

NEAR supports batching multiple actions into a single atomic transaction. This is useful for creating accounts, deploying contracts, or any sequence that should succeed or fail together:

```rust
near.transaction("sub.alice.testnet")
    .create_account()
    .transfer(NearToken::near(5))
    .add_full_access_key(new_public_key)
    .deploy(wasm_bytes)
    .call("init")
        .args(serde_json::json!({ "owner": "alice.testnet" }))
    .send()
    .await?;
```

## Typed Contract Interfaces

Tired of stringly-typed method names and `serde_json::json!` everywhere? Define a trait for your contract and get compile-time checking:

```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;          // view method

    #[call]
    fn increment(&mut self);              // change method
}

// Now you get autocomplete and type errors at compile time
let counter = near.contract::<Counter>("counter.testnet");
let count = counter.get_count().await?;
counter.increment().await?;
```

## Signers

Different situations call for different key management. near-kit supports several approaches:

| Signer | When to use it |
|--------|----------------|
| `InMemorySigner` | Scripts and bots with a hardcoded or loaded key |
| `FileSigner` | Local development — reads from `~/.near-credentials` |
| `EnvSigner` | CI/CD pipelines via `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` |
| `RotatingSigner` | High-throughput apps that need multiple keys to avoid nonce conflicts |
| `KeyringSigner` | Desktop apps using the system keychain (requires `keyring` feature) |

## Token Standards

Working with fungible or non-fungible tokens? near-kit includes helpers for NEP-141 and NEP-171:

```rust
// Fungible tokens
let usdc = near.ft("usdc.testnet")?;
let balance = usdc.balance_of("alice.testnet").await?;
println!("Balance: {}", balance);  // "1.50 USDC"

// Non-fungible tokens
let nft = near.nft("nft.testnet")?;
if let Some(token) = nft.token("token-123").await? {
    println!("Owner: {}", token.owner_id);
}
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `sandbox` | Local testing with [near-sandbox](https://crates.io/crates/near-sandbox) |
| `keyring` | System keyring integration for desktop apps |

## Documentation

For the full API reference, see [docs.rs/near-kit](https://docs.rs/near-kit).

## License

MIT
