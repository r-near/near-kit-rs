# near-kit-sandbox

A local NEAR node in Docker, for tests that use
[`near-kit`](https://crates.io/crates/near-kit).

[API docs](https://docs.rs/near-kit-sandbox) · [Changelog](CHANGELOG.md)

`Sandbox` starts a `nearprotocol/sandbox` container through testcontainers and
hands back an ordinary `near_kit::SigningNear` for its root account, so test
code reads like production code. Accounts are created the normal way, with
`Action::create_account()` transactions from the root account.

## Setup

Add the crates as development dependencies:

```sh
cargo add --dev near-kit near-kit-sandbox
cargo add --dev tokio --features macros,rt-multi-thread
```

Docker must be installed and running. testcontainers finds it through the
default socket; when Docker listens elsewhere (Colima, Rancher Desktop and
similar setups), set `DOCKER_HOST` to the active context's endpoint:

```sh
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

The first start pulls the image, which can take a while.

## Starting a sandbox

`Sandbox::builder().shared()` starts one process-wide sandbox on first use and
returns the same one afterwards; tests sharing it must use distinct account
names. `Sandbox::builder().fresh()` starts a new sandbox with clean state,
stopped when the last clone of it drops.

`sandbox.root_account()` is a `SigningNear` for the root account (`sandbox` by
default), funded at genesis. Sign as another account with
`root.near().with_signer(signer)`. In a test, the same code goes in a
`#[tokio::test]` function:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::{NearToken, account};
use near_kit_sandbox::Sandbox;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();

    // Create and fund an account with its own key.
    let alice_key = SecretKey::generate_ed25519();
    root.transaction(account!("alice.sandbox"))
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(100)))
        .add_action(Action::add_full_access_key(alice_key.public_key()))
        .await?
        .result()?;

    let alice = root
        .near()
        .with_signer(InMemorySigner::new(account!("alice.sandbox"), alice_key));
    let balance = alice.near().balance(alice.account_id()).await?;
    assert!(balance.available > NearToken::from_near(99));
    Ok(())
}
```

## Changing chain state directly

- `set_balance` sets an account's balance and returns once the new balance is
  visible.
- `patch_state` writes raw state records (accounts, access keys, contract code
  or storage). The patch applies asynchronously, so poll for the state you
  wrote before relying on it.
- `fast_forward` moves the chain ahead by a number of blocks, for code that
  depends on block height or epochs.

## Versions and configuration

```rust,no_run
use near_kit::account;
use near_kit_sandbox::Sandbox;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder()
        .version("2.13.4") // a nearcore release; the default is the one this crate is tested against
        .root_account(account!("local"))
        .chain_id("localnet") // "mainnet" and "testnet" are rejected
        .fresh()
        .await?;
    assert_eq!(sandbox.root_account().account_id(), "local");
    Ok(())
}
```

Options passed to `shared()` apply only to the call that starts the shared
sandbox. Set `NEAR_SANDBOX_IMAGE` to use another image, as `name` or
`name:tag` (for example `my-registry/near-sandbox:custom`). The image must
define a Docker health check, which is how the crate knows the node is ready.

## Features

- `tracing`: sandbox lifecycle events, and near-kit's `tracing` feature.
- `integration-tests`: this repository's own Docker-backed tests and examples.
  Applications don't need it.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
