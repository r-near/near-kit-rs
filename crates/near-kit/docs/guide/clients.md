# Clients and networks

There are two client types. [`Near`](crate::Near) reads chain state and
submits transactions that are already signed. [`SigningNear`](crate::SigningNear)
is a `Near` plus a signer for one account, and it also builds, signs and
sends transactions.

## Reading and sending

Build a `Near` for a network, then attach a signer to get a `SigningNear`
for an account:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::InMemorySigner;
use near_kit::*;

# async fn example() -> Result<(), Error> {
let near = Near::testnet().build();

// Reads need no account.
let balance = near.balance(account!("alice.testnet")).await?;

// Sends need a signer.
let alice = near.with_signer(InMemorySigner::new(
    account!("alice.testnet"),
    "ed25519:...".parse()?,
));
alice
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?.result()?;

// A SigningNear reads through the Near it was made from.
let balance = alice.near().balance(alice.account_id()).await?;
# Ok(())
# }
```

`SigningNear` has no read methods of its own and doesn't deref to `Near`;
`alice.near()` is the way to read. It does implement
[`Viewer`](crate::Viewer) as well as [`TxSender`](crate::TxSender), so a
typed contract client built from it can both view and call.

For several reads in a row, bind the read-only client once. `near()` only
borrows a field, so this costs nothing:

```rust,no_run
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let near = alice.near();
let chain_id = near.chain_id();
let balance = near.balance(alice.account_id()).await?;
let bob = near.balance(account!("bob.testnet")).await?;
# let _ = (chain_id, balance, bob);
# Ok(())
# }
```

Both clients implement `AsRef<Near>`, so a helper that only reads can take
`client: impl AsRef<Near>`, call `client.as_ref()`, and accept either a
`Near` or a `SigningNear`:

```rust,no_run
use near_kit::*;
use near_kit::rpc::Finality;

async fn height_and_balance(
    client: impl AsRef<Near>,
    account_id: &AccountId,
) -> Result<(u64, NearToken), Error> {
    let near = client.as_ref();
    let height = near.rpc().block(Finality::Final).await?.header.height;
    let balance = near.balance(account_id).at(height).await?;
    Ok((height, balance.available))
}

# async fn example(alice: SigningNear) -> Result<(), Error> {
let near = Near::testnet().build();
height_and_balance(&near, alice.account_id()).await?;
height_and_balance(&alice, alice.account_id()).await?;
# Ok(())
# }
```

Both types are cheap to clone and are `Send + Sync`: a `Near` holds its RPC
client in an `Arc`, and every `SigningNear` made from it shares that
connection. Build one `Near` per endpoint at startup and derive a
`SigningNear` per account from it, rather than building a client per
request:

```rust,no_run
use near_kit::signer::InMemorySigner;
use near_kit::*;

# fn example(alice_key: signer::SecretKey, bob_key: signer::SecretKey) {
let near = Near::mainnet().build();
let alice = near.with_signer(InMemorySigner::new(account!("alice.near"), alice_key));
let bob = near.with_signer(InMemorySigner::new(account!("bob.near"), bob_key));

// Hand a clone to each task.
tokio::spawn({
    let alice = alice.clone();
    async move {
        let _ = alice.near().balance(alice.account_id()).await;
    }
});
# }
```

For CI and scripts, [`SigningNear::from_env`](crate::SigningNear::from_env)
builds the whole thing from environment variables: `NEAR_ACCOUNT_ID` and
`NEAR_PRIVATE_KEY` (required), `NEAR_NETWORK` (`mainnet`, `testnet` or an
RPC URL; `testnet` if unset), `NEAR_CHAIN_ID` (required with an RPC URL,
an override for `mainnet` and `testnet`) and `NEAR_MAX_NONCE_RETRIES`.

## Endpoints

`Near::mainnet()` and `Near::testnet()` point at FastNEAR's free public
endpoints, `https://free.rpc.fastnear.com` and
`https://test.rpc.fastnear.com`. They are rate-limited and meant for
development and light use. For production traffic, use an RPC provider or
a node of your own with [`Near::custom`](crate::Near::custom), which takes
the URL and the network's chain ID:

```rust
use near_kit::Near;

let near = Near::custom("https://rpc.mainnet.example.com", "mainnet").build();
assert!(near.chain_id().is_mainnet());
assert_eq!(near.rpc().url(), "https://rpc.mainnet.example.com");
```

Use the network's real chain ID (`mainnet`, `testnet`, or your network's);
the [nonces chapter](crate::guide::nonces) explains why.

### API keys and fallback endpoints

Providers that want an API key in a header get it on their
[`RpcEndpoint`](crate::rpc::RpcEndpoint), with
[`bearer`](crate::rpc::RpcEndpoint::bearer) for `Authorization: Bearer ..`
or [`header`](crate::rpc::RpcEndpoint::header) for any other. A URL alone
converts into an endpoint, so `Near::custom` takes either. Add other
providers with [`fallback`](crate::NearBuilder::fallback):

```rust
use near_kit::Near;
use near_kit::rpc::RpcEndpoint;

let near = Near::custom(
    RpcEndpoint::new("https://rpc.mainnet.example.com").bearer("my-api-key"),
    "mainnet",
)
.fallback(RpcEndpoint::new("https://rpc.other.example.com").header("x-api-key", "other-key"))
.fallback("https://free.rpc.fastnear.com")
.build();
```

Every call starts at the first endpoint. When it fails with an error that
[`is_retryable`](crate::rpc::RpcError::is_retryable), such as a refused
connection, a timeout, a 5xx or a node that answers `NODE_IS_SYNCING` or
`BUSY`, the client tries the next endpoint at once, and waits the retry
delay only after every endpoint has failed; see
[retries](crate::guide::errors#retries). Any other error, such as an
unknown account, comes back at once, since another node would give the same
answer. An endpoint's headers go to that endpoint only, and `tracing` spans,
events and `Debug` output show the URL's scheme and host and the header
names, never the header values. All endpoints should serve the same network.

Some providers take the key in the URL instead; pass that URL. `tracing`
and `Debug` output show only the URL's scheme and host, never its path,
query string or credentials.

### Proxies and timeouts

A proxy or a request timeout is configured on the HTTP client. Build a
`reqwest::Client` and wrap it with
[`ReqwestTransport::with_client`](crate::rpc::ReqwestTransport::with_client).
[`near_kit::reqwest`](crate::reqwest) is the reqwest near-kit is built
with, so the client's version always matches:

```rust
use std::time::Duration;

use near_kit::Near;
use near_kit::reqwest;
use near_kit::rpc::ReqwestTransport;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let http = reqwest::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()?;

let near = Near::custom("https://rpc.mainnet.example.com", "mainnet")
    .transport(ReqwestTransport::with_client(http))
    .build();
# Ok(())
# }
```

The client's default headers go to every endpoint, so keep provider keys
on their endpoints instead. An endpoint header replaces a default header of
the same name.

Platforms whose HTTP stack isn't reqwest implement
[`RpcTransport`](crate::rpc::RpcTransport) themselves; see the
[platforms](crate::guide::platforms) and [testing](crate::guide::testing)
chapters.

## Client settings

[`NearBuilder`](crate::NearBuilder) collects the settings that apply to
every call through a client:

| Setting | Default | Covers |
|---|---|---|
| [`fallback`](crate::NearBuilder::fallback) | None | Other endpoints to try when one fails with a retryable error |
| [`transport`](crate::NearBuilder::transport) | reqwest (`wasi:http` on WASI with `wasi-http`) | How HTTP requests are made |
| [`retry_config`](crate::NearBuilder::retry_config) | 3 retries, 500 ms doubling | Re-sending a request after a transient failure |
| [`max_nonce_retries`](crate::NearBuilder::max_nonce_retries) | 3 | Re-signing a send after a nonce or expiry rejection |
| Chain ID, read with [`Near::chain_id`](crate::Near::chain_id) | From the preset, or the second argument of [`Near::custom`](crate::Near::custom) | The nonce cache key for a backend that can't report its genesis hash |

The [errors chapter](crate::guide::errors#retries) explains what each kind
of retry covers.

Two settings belong to the sender rather than the connection, and are set
on a `SigningNear`, returning a new one for the same account:

- [`wait_until::<W>()`](crate::SigningNear::wait_until) picks how long sends
  wait, and with it the response type. The default is `Final`; see the
  [finality chapter](crate::guide::finality).
- [`max_nonce_retries(n)`](crate::SigningNear::max_nonce_retries) overrides
  the builder's value for this sender.

[`with_signer(signer)`](crate::SigningNear::with_signer) on a `SigningNear`
keeps both and changes only the account: a relayer set to `Included` signs
as another account at `Included`. Through the read-only client,
`alice.near().with_signer(signer)`, the wait level starts over at `Final`.

## Other RPC methods

`Near` keeps the reads that add something on top of the RPC:
[`balance`](crate::Near::balance) (computes the spendable amount),
[`account_exists`](crate::Near::account_exists),
[`view`](crate::Near::view) (decodes the result) and
[`tx_status`](crate::Near::tx_status). Everything else is a method on
[`near.rpc()`](crate::rpc::RpcClient), with typed responses:

```rust,no_run
use near_kit::rpc::Finality;
use near_kit::*;

# async fn example(near: &Near) -> Result<(), Error> {
let rpc = near.rpc();
let account = rpc.view_account(&account!("alice.testnet"), Finality::Final).await?;
let keys = rpc.view_access_key_list(&account!("alice.testnet"), Finality::Final).await?;
let code = rpc.view_code(&account!("guestbook.testnet"), Finality::Final).await?;
let status = rpc.status().await?;
let validators = rpc.validators(None).await?;
# Ok(())
# }
```

For a method near-kit has no wrapper for, send the JSON yourself with
[`request`](crate::rpc::RpcClient::request). It gets the same retries:

```rust,no_run
use near_kit::*;

# async fn example(near: &Near) -> Result<(), Error> {
let genesis: serde_json::Value = near
    .rpc()
    .request("genesis_config", serde_json::json!([]))
    .await?;
# Ok(())
# }
```
