# near-kit-sandbox

Docker-backed local NEAR sandbox support for [`near-kit`](https://crates.io/crates/near-kit).

[API documentation](https://docs.rs/near-kit-sandbox) · [Changelog](CHANGELOG.md)

This companion crate owns the Docker/testcontainers lifecycle that previously
lived behind near-kit's `sandbox` feature. It starts a `nearprotocol/sandbox`
container and returns a normal `near_kit::SigningNear` for its root account.

## Setup

Docker must be installed and running. Add the crate as a development
dependency:

```toml
[dev-dependencies]
near-kit = "0.21"
near-kit-sandbox = "0.21"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

When Docker uses a non-default socket (for example Rancher Desktop), set
`DOCKER_HOST` to the active context's endpoint before running tests:

```sh
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

```rust
use near_kit::{account, NearToken, protocol::Action, signer::SecretKey};
use near_kit_sandbox::Sandbox;

#[tokio::test]
async fn creates_an_account() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.client();
    let alice_key = SecretKey::generate_ed25519();

    root.transaction(account!("alice.sandbox"))
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(100)))
        .add_action(Action::add_full_access_key(alice_key.public_key()))
        .await?;

    Ok(())
}
```

Use `Sandbox::builder().fresh()` when a test needs isolated chain state. Use
`Sandbox::builder().shared()` to reuse one process-wide sandbox and reduce startup
cost. The shared instance persists until process exit; tests sharing it must use
unique accounts or otherwise coordinate state.

`sandbox.client()` returns a `SigningNear` for the sandbox root account. Sign
as another account with `sandbox.client().near().with_signer(signer)`. `Sandbox::set_balance` and
`Sandbox::fast_forward` expose sandbox-owned test controls.

For custom configuration:

```rust
let sandbox = Sandbox::builder()
    .version("2.13.4")
    .root_account(account!("local"))
    .chain_id("localnet")
    .fresh()
    .await?;
```

Set `NEAR_SANDBOX_IMAGE` to override the Docker image and tag, for example
`my-registry/near-sandbox:custom`. The selected image must provide a Docker
health check.

## Features

- `tracing` enables sandbox lifecycle events and near-kit tracing.
- `integration-tests` compiles this repository's Docker-backed integration
  suite and examples; applications normally do not need it.

## License

Licensed under either Apache-2.0 or MIT, at your option.
