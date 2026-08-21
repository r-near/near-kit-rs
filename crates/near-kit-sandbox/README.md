# near-kit-sandbox

Docker-backed local NEAR sandbox support for [`near-kit`](https://crates.io/crates/near-kit).

[API documentation](https://docs.rs/near-kit-sandbox) · [Changelog](CHANGELOG.md)

This companion crate owns the Docker/testcontainers lifecycle that previously
lived behind near-kit's `sandbox` feature. It starts a `nearprotocol/sandbox`
container and returns a normal signed `near_kit::Near` client.

## Setup

Docker must be installed and running. Add the crate as a development
dependency:

```toml
[dev-dependencies]
near-kit = "0.18"
near-kit-sandbox = "0.18"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use near_kit::{NearToken, signer::SecretKey};
use near_kit_sandbox::SandboxConfig;

#[tokio::test]
async fn creates_an_account() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = SandboxConfig::fresh().await?;
    let near = sandbox.client();
    let alice_key = SecretKey::generate_ed25519();

    near.transaction("alice.sandbox")
        .create_account()
        .transfer(NearToken::from_near(100))
        .add_full_access_key(alice_key.public_key())
        .send()
        .await?;

    Ok(())
}
```

Use `SandboxConfig::fresh()` when a test needs isolated chain state. Use
`SandboxConfig::shared()` to reuse one process-wide sandbox and reduce startup
cost. The shared instance persists until process exit; tests sharing it must use
unique accounts or otherwise coordinate state.

`sandbox.client()` and `Near::sandbox(&sandbox)` both return a configured
`Near` signed by the sandbox root account. `Sandbox::set_balance` and
`Sandbox::fast_forward` expose sandbox-owned test controls.

For custom configuration:

```rust
let sandbox = SandboxConfig::builder()
    .version("2.13.0-rc.2")
    .root_account("local")
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
