# near-kit-rs

A clean, ergonomic Rust client for NEAR Protocol.

> ⚠️ **Work in Progress**: This is a ground-up rewrite, not yet ready for production use.

## Why?

The existing `near-api-rs` has grown organically and has some pain points:
- Fragmented entry points (`Contract`, `Tokens`, `Account`, etc.)
- Verbose builder patterns
- Network specified at execution time instead of configuration
- Inherited types from `near-primitives` that are heavier than needed

**near-kit-rs** is a fresh start with:
- Single entry point: `Near` client
- Ergonomic, fluent API
- Hand-rolled types based on actual RPC responses
- Minimal dependencies

## Quick Example

```rust
use near_kit::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Configure once with inline credentials
    let near = Near::testnet()
        .credentials("ed25519:3D4YudUahN1nawWogh8p...", "alice.testnet")?
        .build();
    
    // Simple operations
    let balance = near.balance("alice.testnet").await?;
    println!("Balance: {}", balance);  // "10.5 NEAR"
    
    near.transfer("bob.testnet", "1 NEAR").await?;
    
    near.call("counter.testnet", "increment")
        .gas("50 Tgas")
        .await?;
    
    // Multi-action transactions
    let new_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
    let wasm_code = std::fs::read("contract.wasm")?;
    
    near.transaction("new.alice.testnet")
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(new_key)
        .deploy(wasm_code)
        .call("init").args(serde_json::json!({ "owner": "alice.testnet" }))
        .send()
        .await?;
    
    Ok(())
}
```

## Signers

near-kit-rs provides several signer implementations for different use cases:

```rust
use near_kit::{Near, InMemorySigner, FileSigner, EnvSigner, RotatingSigner, SecretKey};

// InMemorySigner - Single key in memory (most common)
let signer = InMemorySigner::new(
    "alice.testnet",
    "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr"
)?;

// FileSigner - Load from ~/.near-credentials/{network}/{account}.json
let signer = FileSigner::new("testnet", "alice.testnet")?;

// EnvSigner - Load from NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY env vars
let signer = EnvSigner::new()?;

// RotatingSigner - Multiple keys with round-robin rotation (for high-throughput bots)
let keys = vec![
    SecretKey::generate_ed25519(),
    SecretKey::generate_ed25519(),
    SecretKey::generate_ed25519(),
];
let signer = RotatingSigner::new("bot.testnet", keys)?;

// Use any signer with the Near client
let near = Near::testnet().signer(signer).build();
```

## Design Principles

1. **Single entry point**: Everything hangs off `Near`
2. **Configure once**: Network and signer set at client creation
3. **Type-safe but ergonomic**: Accept `"5 NEAR"` strings while maintaining type safety
4. **Explicit units**: No ambiguous amounts - must specify `NEAR`, `yocto`, `Tgas`, etc.
5. **Progressive disclosure**: Simple things are simple, advanced options available when needed

## Features

- **Read operations**: `balance()`, `account()`, `account_exists()`, `view()`, `access_keys()`
- **Write operations**: `transfer()`, `call()`, `deploy()`, `add_full_access_key()`, `delete_key()`
- **Multi-action transactions**: `transaction()` builder for atomic operations
- **Block references**: Query at specific block heights, hashes, or finality levels
- **Retry logic**: Automatic retry with exponential backoff for transient failures

## Documentation

See [SPEC.md](./SPEC.md) for the full API specification.

## License

MIT OR Apache-2.0
