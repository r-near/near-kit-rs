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
    // Configure once
    let near = Near::testnet()
        .signer(SecretKeySigner::from_seed_phrase("your seed phrase...", None)?)
        .default_account("alice.testnet")
        .build();
    
    // Simple operations
    let balance = near.balance("alice.testnet").await?;
    println!("Balance: {}", balance);  // "10.5 NEAR"
    
    near.transfer("bob.testnet", "1 NEAR").await?;
    
    near.call("counter.testnet", "increment")
        .gas("50 Tgas")
        .await?;
    
    // Batch transactions
    near.batch("new.alice.testnet")
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(public_key)
        .deploy(wasm_code)
        .call("init").args(json!({ "owner": "alice.testnet" }))
        .send()
        .await?;
    
    Ok(())
}
```

## Design Principles

1. **Single entry point**: Everything hangs off `Near`
2. **Configure once**: Network and signer set at client creation
3. **Type-safe but ergonomic**: Accept `"5 NEAR"` strings while maintaining type safety
4. **Explicit units**: No ambiguous amounts - must specify `NEAR`, `yocto`, `Tgas`, etc.
5. **Progressive disclosure**: Simple things are simple, advanced options available when needed

## Documentation

See [SPEC.md](./SPEC.md) for the full specification.

## License

MIT OR Apache-2.0
