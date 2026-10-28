# near-kit

A Rust client for NEAR Protocol: read chain state, send transactions, and call
contracts through typed clients.

[![Crates.io](https://img.shields.io/crates/v/near-kit.svg)](https://crates.io/crates/near-kit)
[![Documentation](https://docs.rs/near-kit/badge.svg)](https://docs.rs/near-kit)
[![CI](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/r-near/near-kit-rs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/r-near/near-kit-rs/graph/badge.svg)](https://codecov.io/gh/r-near/near-kit-rs)
[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue.svg)](https://github.com/r-near/near-kit-rs)

[API docs](https://docs.rs/near-kit) · [Guide](https://docs.rs/near-kit/latest/near_kit/guide/index.html) · [Examples](#examples) · [Upgrading](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md) · [Changelog](https://github.com/r-near/near-kit-rs/blob/main/crates/near-kit/CHANGELOG.md)

## Install

```sh
cargo add near-kit
cargo add near-kit --features contracts   # typed contract clients
cargo add tokio --features macros,rt-multi-thread
```

## Quick start

```rust,no_run
use near_kit::protocol::Action;
use near_kit::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Reads need no signer and read the latest final block by default.
    let near = Near::testnet().build();
    let guestbook = account!("guestbook.near-examples.testnet");
    let balance = near.balance(&guestbook).await?;
    let messages: u64 = near.view(&guestbook, "total_messages").await?;
    println!("{} available, {messages} messages", balance.available);

    // Sends need a signer. This one reads NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY.
    let alice = SigningNear::from_env()?;
    let outcome = alice
        .transaction(account!("bob.testnet"))
        .add_action(Action::transfer(NearToken::from_millinear(100)))
        .await?;

    // `Err` means the transaction didn't run. A failed action is `Ok(outcome)`.
    if outcome.is_failure() {
        println!("transfer failed: {:?}", outcome.failure_message());
    }
    Ok(())
}
```

## How it fits together

- `Near` reads. `near.with_signer(signer)` gives a `SigningNear`, which reads and sends.
- A transaction is a receiver plus `Action`s: `alice.transaction(id).add_action(..).await`.
  It waits for `Final` execution unless `alice.wait_until::<W>()` picks another level.
- Reads are `View` values: `.args(..)`, `.at(block)`, then `.await`.
- `#[near_kit::contract]` turns a trait describing a contract into a typed client.
- Everything above is written against two traits, `Viewer` and `TxSender`, so it
  also runs on your own backend: a mock, a gateway, or a send queue.

## Guides

- [Clients and networks](https://docs.rs/near-kit/latest/near_kit/guide/clients/index.html)
- [Transactions and actions](https://docs.rs/near-kit/latest/near_kit/guide/transactions/index.html)
- [Finality and wait levels](https://docs.rs/near-kit/latest/near_kit/guide/finality/index.html)
- [Nonces and concurrent sends](https://docs.rs/near-kit/latest/near_kit/guide/nonces/index.html)
- [Errors and retries](https://docs.rs/near-kit/latest/near_kit/guide/errors/index.html)
- [Meta-transactions and relayers](https://docs.rs/near-kit/latest/near_kit/guide/meta_transactions/index.html)
- [Offline signing](https://docs.rs/near-kit/latest/near_kit/guide/offline_signing/index.html)
- [Typed contracts and tokens](https://docs.rs/near-kit/latest/near_kit/guide/contracts_and_tokens/index.html)
- [Testing: mocks and the sandbox](https://docs.rs/near-kit/latest/near_kit/guide/testing/index.html)
- [Keys and signers](https://docs.rs/near-kit/latest/near_kit/guide/keys/index.html)
- [Platforms: browser, WASI, contracts, tracing](https://docs.rs/near-kit/latest/near_kit/guide/platforms/index.html)

## Examples

Each example's header says what it shows and how to run it. Sandbox examples
start a local node in Docker.

| Example | Shows | Runs on |
|---|---|---|
| [`quickstart`](crates/near-kit/examples/quickstart.rs) | Balances, views, account checks; a write if credentials are set | testnet |
| [`typed_contract`](crates/near-kit-sandbox/examples/typed_contract.rs) | `#[contract]` views and calls, batching a typed call into a transaction | sandbox |
| [`tokens`](crates/near-kit-sandbox/examples/tokens.rs) | Fungible token storage deposit, transfer, balance and decimals | sandbox |
| [`relayer`](crates/near-kit-sandbox/examples/relayer.rs) | A signed delegate action sent as base64, checked and submitted by a relayer | sandbox |
| [`offline_signing`](crates/near-kit/examples/offline_signing.rs) | Building and signing a transaction with no client; NEP-413 messages | none |
| [`mock_backend`](crates/near-kit/examples/mock_backend.rs) | A typed client and the token client over a mock `Viewer` and `TxSender` | none |
| [`rotating_signer`](crates/near-kit-sandbox/examples/rotating_signer.rs) | Concurrent sends from one account through several keys | sandbox |
| [`sequential_sends`](crates/near-kit-sandbox/examples/sequential_sends.rs) | Ordered sends per key with `into_per_key_signers` | sandbox |
| [`global_contracts`](crates/near-kit-sandbox/examples/global_contracts.rs) | Publishing a global contract and deploying from it | sandbox |

## Crates

| Crate | Use it for |
|---|---|
| [`near-kit`](https://docs.rs/near-kit) | The client: everything on this page |
| [`near-kit-primitives`](crates/near-kit-primitives/README.md) | Types, keys, signing and transaction building with no RPC client or async runtime, for contracts, signing services and wasm |
| [`near-kit-sandbox`](crates/near-kit-sandbox/README.md) | Tests against a local NEAR node in Docker |
| [`near-kit-macros`](https://docs.rs/near-kit-macros) | The `#[contract]` macro; use it through the `contracts` feature |

## Feature flags

| Feature | Default | Enables |
|---|---|---|
| `rpc` | yes | `Near`, `SigningNear`, the token clients and the HTTP transport. Turn off for offline signing |
| `contracts` | no | `#[near_kit::contract]` |
| `file-signer` | no | Loading keys from `~/.near-credentials` |
| `keyring` | no | Loading keys from the system keyring |
| `mnemonic` | no | BIP-39 seed phrases and SLIP-10 key derivation |
| `tracing` | no | `tracing` spans and events for RPC calls and transactions |
| `wasi-http` | no | A `wasi:http` transport for `wasm32-wasip2` (implies `rpc`) |
| `js` | no | Entropy from the JS host (browser, Node, Deno) on `wasm32-unknown-unknown` |

## RPC endpoints

`Near::mainnet()` and `Near::testnet()` use FastNEAR's free tier
(`https://free.rpc.fastnear.com` and `https://test.rpc.fastnear.com`), which is
rate-limited. For production traffic, point the client at your own endpoint
with `Near::custom`. To send an API key or other headers, build a
`reqwest::Client` with them and pass `ReqwestTransport::with_client(client)` to
`NearBuilder::transport`.

```rust
use near_kit::Near;

fn main() {
    let near = Near::custom("https://rpc.example.com", "mainnet").build();
    assert_eq!(near.chain_id().as_str(), "mainnet");
}
```

## MSRV and versioning

The minimum supported Rust version is 1.95. Before 1.0, a minor release can
break the API. near-kit, near-kit-primitives and near-kit-sandbox are released
together with the same version number; near-kit-macros has its own.

## Upgrading

[MIGRATION.md](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md)
covers upgrading from 0.20.1: a lookup table from each old item to its
replacement, then the changes by task. It links the guides for older releases.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
