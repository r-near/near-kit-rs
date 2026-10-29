# near-kit-sandbox

A local NEAR node in Docker, for tests that use
[`near-kit`](https://crates.io/crates/near-kit).

[API docs](https://docs.rs/near-kit-sandbox/0.30.0-rc.1) · [Changelog](CHANGELOG.md)

```sh
cargo add --dev near-kit@0.30.0-rc.1 near-kit-sandbox@0.30.0-rc.1
cargo add --dev tokio --features macros,rt-multi-thread
```

Docker must be running. When it listens somewhere other than the default
socket (Colima, Rancher Desktop and similar setups), point `DOCKER_HOST` at
the active context's endpoint:

```sh
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

For code that is configured with an RPC URL and chain ID (such as
`SigningNear::from_env`, through `NEAR_NETWORK` and `NEAR_CHAIN_ID`), the
sandbox's are `sandbox.root_account().near().rpc().url()` and
`sandbox.root_account().near().chain_id()`.

The API docs cover starting a sandbox, creating accounts, changing chain
state, and choosing a nearcore version or image.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
