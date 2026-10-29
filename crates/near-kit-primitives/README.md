# near-kit-primitives

NEAR Protocol types, keys, signing, and transaction construction — the offline
core of [`near-kit`](https://crates.io/crates/near-kit), with no RPC client, no
async runtime, and no HTTP stack.

`near-kit` depends on this crate and re-exports every item at the same path
under `near_kit::` (`near_kit_primitives::protocol::Action` is
`near_kit::protocol::Action`), so code using both sees one set of types. Depend
on this crate directly when you only need the types: wasm contracts, signing
services, indexers, or targets without a network stack.

```toml
[dependencies]
near-kit-primitives = "0.20"
```

```rust
use near_kit_primitives::protocol::{Action, Transaction};
use near_kit_primitives::signer::SecretKey;
use near_kit_primitives::{CryptoHash, NearToken};

let secret = SecretKey::generate_ed25519();
let tx = Transaction::new(
    "alice.testnet".parse()?,
    secret.public_key(),
    1,
    "bob.testnet".parse()?,
    CryptoHash::ZERO, // a recent block hash, fetched elsewhere
    vec![Action::transfer(NearToken::from_near(1))],
);
let bytes = tx.sign(&secret).to_bytes();
```

## Modules

| Module | Contents |
|--------|----------|
| crate root | `AccountId`, `NearToken`, `Gas`, `CryptoHash` |
| `protocol` | Actions, transactions, delegate actions, execution errors |
| `signer` | Keys, signatures, `Signer`, `InMemorySigner`, `RotatingSigner` |
| `transaction` | `FunctionCall` and the typed wait levels |
| `rpc` | JSON-RPC response views, `BlockReference`, `Finality` |
| `standards` | NEP-413 message signing and verification |

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `mnemonic` | No | BIP-39 seed phrases and SLIP-10 hierarchical key derivation |
| `js` | No | JS-host entropy backend (`getrandom`'s `wasm_js`) for `wasm32-unknown-unknown` |
| `tracing` | No | `tracing` events (a WARN for unrecognized RPC error variants) |

`near-kit`'s `mnemonic`, `js`, and `tracing` features turn on the same-named
features here.

## License

Licensed under either Apache-2.0 or MIT, at your option.
