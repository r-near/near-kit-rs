# Platforms and features

Each place near-kit runs is a target plus Cargo features:

| Where | Target | Features |
|---|---|---|
| Server, CLI, tests | native | default (`rpc`), plus any of `contracts`, `tracing`, `mnemonic`, `file-signer`, `keyring` |
| Browser, Node, Deno | `wasm32-unknown-unknown` | `default-features = false`, `js`, `rpc` |
| WASI component with `wasi:http` | `wasm32-wasip2` | `default-features = false`, `wasi-http` |
| WASI host without `wasi:http` | `wasm32-wasip2` | `default-features = false`, `rpc`, and your own transport |
| Smart contract, other non-JS wasm | `wasm32-unknown-unknown` | `near-kit-primitives`, a `getrandom` backend |
| Signing only, no network | any | `default-features = false` |

`contracts`, `mnemonic` and `tracing` work on every target. `file-signer`
and `keyring` need an operating system's filesystem or credential store, so
leave them off in the browser and in WASI.

## Browsers and other JS hosts

```sh
cargo add near-kit --no-default-features --features js,rpc
```

On `wasm32-unknown-unknown`, `rpc` sends requests with the host's `fetch`
(reqwest's wasm backend) and waits between retries with the host's timers.
`js` gives key generation and NEP-413 nonces the host's
`crypto.getRandomValues`. Everything in the other chapters works the same,
with three differences:

- **Futures aren't `Send`.** Browser APIs are single-threaded, so the
  `Send` bounds disappear on this target. Spawn with
  `wasm_bindgen_futures::spawn_local` rather than `tokio::spawn`. In your
  own generic code, bound on [`MaybeSend`](crate::MaybeSend) and
  [`MaybeSync`](crate::MaybeSync) instead of `Send` and `Sync`; they are
  `Send` and `Sync` on every other target, so the code still builds there.
- **CORS.** The browser only lets the page read responses from an RPC
  endpoint that allows its origin. A provider's endpoint may need the
  origin registered; your own node or proxy needs the CORS headers.
- **Keys.** Anything in the page can be read by its user and by scripts
  running in it. Sign with keys that belong to the user (generated in the
  page, or from a seed phrase they enter), and keep keys that hold your
  funds on a server. A server-side relayer that pays for the user's
  delegate actions is described in the
  [meta-transactions chapter](crate::guide::meta_transactions).

A transport written for the browser, for example over `web_sys` directly,
implements [`RpcTransport`](crate::rpc::RpcTransport) without `Send`, for
the same reason.

## Wasm without a JS host

`wasm32-unknown-unknown` outside a JS host (a smart contract, a plugin
runtime, an embedded engine) has no source of randomness that
[`getrandom`](https://docs.rs/getrandom) can find on its own, and near-kit
depends on `getrandom` for key generation and NEP-413 nonces. The build
fails in `getrandom` until you choose a backend with a `--cfg` flag:

- `getrandom_backend="custom"`, and export an `extern "Rust"` function named
  `__getrandom_v03_custom` that fills a buffer from the host's entropy. The
  name says `v03` though the crate is 0.4; that is the symbol it links
  against. The [`getrandom` docs](https://docs.rs/getrandom) give the
  signature.
- `getrandom_backend="unsupported"`, if the code never generates keys or
  NEP-413 nonces. Everything compiles, and asking for randomness returns an
  error at run time.

Set the flag for that target only, for example in `.cargo/config.toml`:

```toml
[target.wasm32-unknown-unknown]
rustflags = ['--cfg', 'getrandom_backend="unsupported"']
```

A `RUSTFLAGS` environment variable replaces these settings, so a build
script or tool that sets `RUSTFLAGS` has to include the flag itself. Leave
`js` off here: it assumes a JS host and fails to link or run without one.
Leave `rpc` off too, since its HTTP client needs `fetch`.

## WASI

WASI targets have their own entropy source, so `js` and the `--cfg` flags
above aren't needed. reqwest doesn't build for WASI, so the RPC layer
needs a transport from somewhere else.

### With `wasi:http`

```sh
cargo add near-kit --no-default-features --features wasi-http
```

`wasi-http` implies `rpc` and adds
`WasiHttpTransport`, which `Near` uses by default on `wasm32-wasip2`. The
host must provide the `wasi:http` interface: `wasmtime run -S http`, or any
runtime for the `wasi:http/proxy` world. Three things behave differently
from native:

- **Requests block.** The component waits for each response, so one
  request is in flight at a time, and concurrent calls (several
  `try_join_all` sends, say) run one after another.
- **Retry delays block too**, with `std::thread::sleep`, since there is no
  async runtime in the component.
- **Redirects aren't followed.** reqwest follows up to ten. Point the
  client at the final RPC URL.

`wasi-http` is only for `wasm32-wasip2`: it is ignored on non-WASI targets
and a compile error on earlier WASI targets, which have no `wasi:http`.

### Without `wasi:http`

Some hosts don't offer `wasi:http`, and refuse to instantiate a component
that imports it. Compiling the built-in transport adds that import, so use
`rpc` without `wasi-http`, and give the client whatever HTTP the host does
offer through [`NearBuilder::transport`](crate::NearBuilder::transport):

```sh
cargo add near-kit --no-default-features --features rpc
```

```rust
use near_kit::Near;
use near_kit::rpc::{BoxFuture, RpcError, RpcTransport, TransportResponse};

/// Calls the host's HTTP function. `host_post` stands in for its binding.
struct HostHttp;

# fn host_post(_url: &str, _body: &[u8]) -> Result<(u16, Vec<u8>), String> { Ok((200, Vec::new())) }
impl RpcTransport for HostHttp {
    fn post_json(&self, url: &str, body: Vec<u8>) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        let result = host_post(url, &body);
        Box::pin(async move {
            let (status, body) = result.map_err(|message| RpcError::Network {
                message,
                status_code: None,
                retryable: true,
            })?;
            Ok(TransportResponse::new(status, body))
        })
    }
}

let near = Near::custom("https://rpc.mainnet.example.com", "mainnet")
    .transport(HostHttp)
    .build();
```

Return `Ok` with the status and body for every HTTP response, including
4xx and 5xx, because nearcore sends JSON-RPC errors in error responses and
the client reads them. Return `Err` only when there was no response. In
[`RpcError::Network`](crate::rpc::RpcError::Network), `retryable` decides
whether the client tries again: `true` for timeouts and dropped
connections, `false` for a malformed URL.

On such a target without a transport, `build()` still succeeds, and every
RPC call fails with a non-retryable `RpcError::Network` naming the missing
transport.

## Offline builds

With `default-features = false`, near-kit has no RPC client, no async
runtime and no HTTP stack. What's left is everything that doesn't talk to a
node:

- the types: accounts, tokens, gas, hashes, actions, transactions,
  delegate actions, and the RPC response views;
- keys, signers and signing, including seed phrases with `mnemonic`;
- building transactions with
  [`Transaction::new`](crate::protocol::Transaction::new) and signing them,
  as in the [offline signing chapter](crate::guide::offline_signing#fully-offline);
- [`FunctionCall`](crate::transaction::FunctionCall) and, with `contracts`,
  the `#[contract]` macro, whose generated clients then have no default
  backend;
- NEP-413 [`verify_signature`](crate::standards::nep413::verify_signature);
- [`Viewer`](crate::Viewer) and [`TxSender`](crate::TxSender), for a
  backend of your own.

```sh
cargo add near-kit --no-default-features
```

The same offline core is published as
[`near-kit-primitives`](https://docs.rs/near-kit-primitives), which near-kit
re-exports at the same paths: `near_kit_primitives::protocol::Action` is
`near_kit::protocol::Action`. Depend on it directly in a crate that must
never pull in the RPC layer, such as a smart contract, a signing service
or a library of shared types. Values pass between the two crates without
conversion.

## Smart contracts

A NEAR smart contract can use near-kit-primitives with `contracts` to
describe the contracts it calls, and get the same typed calls as client
code. The contract builds the call, converts it to its parts, and passes
them to near-sdk's promise API:

```sh
cargo add near-kit-primitives --features contracts
```

```rust
use near_kit_primitives::protocol::FunctionCallAction;
use near_kit_primitives::transaction::FunctionCall;
use near_kit_primitives::{Gas, NearToken};

let call = FunctionCall::new("ft_transfer")
    .args(serde_json::json!({ "receiver_id": "bob.near", "amount": "100" }))
    .deposit(NearToken::from_yoctonear(1))
    .gas(Gas::from_tgas(10));

// Encodes the arguments; fails only if they failed to encode.
let FunctionCallAction { method_name, args, gas, deposit } = FunctionCallAction::try_from(call)?;
# Ok::<(), near_kit_primitives::transaction::CodecError>(())
```

A `#[contract]` trait's constructors (`Counter::add(args)`) return the
same `FunctionCall`. Inside the contract the parts become a promise:

```rust,ignore
// `ignore`: near-sdk isn't a dependency of near-kit's doctests. The types
// are converted through their raw values, so they don't depend on matching
// near-sdk's versions of near-token and near-gas.
use near_sdk::{Gas, NearToken, Promise};

let promise = Promise::new("usdc.near".parse().unwrap()).function_call(
    method_name,
    args,
    NearToken::from_yoctonear(deposit.as_yoctonear()),
    Gas::from_gas(gas.as_gas()),
);
```

Contracts build for `wasm32-unknown-unknown` with no JS host, so they need
a `getrandom` backend as described [above](#wasm-without-a-js-host);
`getrandom_backend="unsupported"` is the usual choice, because a contract
doesn't generate keys.

## Tracing

With the `tracing` feature, RPC calls and transactions run inside
[`tracing`](https://docs.rs/tracing) spans: one per RPC method call (with
the method and the endpoint URL, its query string removed in case it holds
an API key), one per view or account read (with the account), one per
transaction send (with the sender, receiver and hash). Events report
retries, failed requests and each step of a send at `DEBUG`, and raw
request and response bodies at `TRACE`.

near-kit doesn't log errors it returns to you at `WARN` or `ERROR`; whether
an error matters is the caller's call, and a probe such as
`account_exists` fails routinely. A `WARN` subscriber therefore stays quiet
in normal operation. The one `WARN` is for something that isn't returned as
an error: an RPC error variant near-kit doesn't recognize, which it decodes
as `Unknown` (usually a node newer than near-kit). Seeing it is a sign to
upgrade.

```rust,no_run
# fn main() {
// With tracing-subscriber, showing near-kit's DEBUG events and WARN elsewhere.
tracing_subscriber::fmt()
    .with_env_filter("warn,near_kit=debug")
    .init();
# }
```

The feature works on every target; in the browser, use a subscriber that
writes to the console.
