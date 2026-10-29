# Agent Instructions for near-kit-rs

A Rust client for NEAR Protocol, with protocol types hand-rolled from real RPC responses.

## Build, Lint, Test Commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features    # CI denies warnings
cargo test -p near-kit                                    # add a test name to filter
cargo test -p near-kit-sandbox --features integration-tests --test integration   # needs Docker
```

CI (`.github/workflows/ci.yml`) also checks `--no-default-features` and the wasm targets.

## Project Structure

```
near-kit-rs/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── near-kit/               # Main library crate (the RPC client)
│   │   ├── src/
│   │   │   ├── lib.rs          # Public API exports
│   │   │   ├── protocol.rs, rpc.rs, signer.rs, transaction.rs, standards.rs
│   │   │   │                   # Public facades: re-export near-kit-primitives + client items
│   │   │   ├── error.rs        # Error, RpcError, KeyStoreError, TimeoutCause
│   │   │   ├── client/         # Near/SigningNear, Tx, RpcClient, transports,
│   │   │   │                   # signer loaders (from_env, from_credentials, from_keyring)
│   │   │   ├── tokens/         # FungibleToken/NonFungibleToken Contract markers + clients
│   │   │   └── standards/      # NEP-413 verify over RPC
│   │   └── examples/
│   ├── near-kit-primitives/    # Offline core: no tokio/reqwest/hyper (CI-enforced)
│   │   └── src/
│   │       ├── lib.rs          # Facades mirror near-kit's (protocol, rpc, signer, ...)
│   │       ├── backend.rs      # Viewer/TxSender, View/Call, Contract, ContractError
│   │       ├── error.rs        # ParseKey/ParseHash/Signer/Codec/Outcome errors
│   │       ├── types/          # AccountId, NearToken, Gas, keys, actions, txs,
│   │       │                   # RPC views, Signer/InMemorySigner/RotatingSigner, NEP-413
│   │       └── transaction/    # FunctionCall
│   ├── near-kit-sandbox/       # Docker-backed sandbox lifecycle and integration tests
│   └── near-kit-macros/        # Proc macros (#[near_kit::contract])
└── lefthook.yml                # Git hooks (clippy, fmt)
```

## Code Style Guidelines

### Imports

Order imports in groups separated by blank lines:
1. Standard library (`std::`)
2. External crates (alphabetical)
3. Crate-local imports (`crate::`, `super::`)

```rust
use std::fmt::{self, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::ParseKeyError;
use crate::types::AccountId;
```

### Error Handling

- Use `thiserror` for all error types
- Implement `From` conversions for error composition

### Documentation

- All public items must have doc comments; each crate sets `#![warn(missing_docs)]` and CI denies warnings. Only the nearcore error mirror (`near-kit-primitives` `types/error.rs`) opts out
- Use `///` for item docs, `//!` for module docs
- Add an example only where use isn't obvious from the signature; task-level examples go in the guide

### Key Patterns

1. **Typed inputs**: Methods take parsed types (`impl Into<AccountId>`, `SecretKey`,
   `NearToken`); callers parse strings at the edge with `FromStr`
2. **Builder pattern**: Only for real configuration (`NearBuilder`, `Sandbox::builder()`)
3. **IntoFuture**: `View`, `Call`, `Tx` and query builders `.await` directly
4. **Backend traits**: Contract clients are generic over `Viewer`/`TxSender`; a
   `Contract` marker (generated or `FungibleToken`) builds them via `near.contract::<T>(id)`

### Serialization

- Use `serde` for JSON (RPC communication)
- Use `borsh` for binary (NEAR's on-chain format)
- When both traits are in scope, fully qualify: `serde::Deserialize::deserialize(...)`

### Where code goes

Anything that needs no RPC client, async runtime, or I/O belongs in
`near-kit-primitives`; `near-kit` re-exports it at the same `near_kit::` path
(add the item to both crates' facade lists). Client-only code stays in
`near-kit`. Primitives' doctests use `near_kit_primitives::` paths and must work
offline; examples that need `Near` go in near-kit's facade module docs.

## Git Hooks (Lefthook)

After cloning, run `bunx lefthook install` to set up hooks:

- **pre-commit**: Runs `cargo clippy --fix` and `cargo fmt` (auto-stages fixes)

## Commit Messages

Use conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`; `!` for breaking).

## Design Philosophy

1. **Two clients**: `Near` reads; `near.with_signer(s)` returns a `SigningNear` that also sends
2. **Configure once**: Network on `Near`, account and wait level on `SigningNear<W>`
3. **One way to do each thing**: Typed values in, no duplicate shortcuts
4. **Explicit units**: Must specify `NEAR`, `yocto`, `Tgas`, etc.
5. **Minimal dependencies**: hand-roll the protocol types; the only NEAR crates used are the small shared value types (`near-account-id`, `near-token`, `near-gas`, `near-global-contracts`). See below

## Dependencies Policy

**Allowed**: tokio, reqwest, serde, borsh, thiserror, bs58, base64, and
well-maintained cryptographic *primitive* crates for the curves/hashes NEAR
uses — e.g. ed25519-dalek, k256 (secp256k1), sha2, sha3, ml-dsa (FIPS-204
ML-DSA-65), bip39, hmac. The bar for a new crypto dependency is that it
implements a standard primitive the protocol requires and is auditable.

Also allowed: the NEAR value-type crates `near-account-id`, `near-token`,
`near-gas` and `near-global-contracts`, re-exported as-is rather than
newtyped.

Also allowed: *platform bindings* for wasm targets, when they are the
canonical thin binding to a host interface and are declared only in that
target's `[target.'cfg(...)']` table — `js-sys`/`gloo-timers`/`getrandom` on
JS-host wasm (`wasm32-unknown-unknown`), and `wasi` (the Bytecode Alliance's
generated WASI 0.2 bindings) on `wasm32-wasip2` for the `wasi:http` transport.
These play the role reqwest/tokio play natively; hand-rolling them would mean
maintaining generated FFI by hand.

Also allowed: `hyper`, only as reqwest's underlying HTTP crate (no features),
to classify transport errors such as an incomplete message. It is gated with
the native transport.

**Not allowed**: near-primitives, near-crypto, near-jsonrpc-client (we hand-roll
the NEAR types/borsh ourselves; this rule forbids the NEAR-specific crates, not
crypto primitives).
