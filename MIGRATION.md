# Migrating to near-kit 0.18

Version 0.18 deliberately narrows near-kit's default and public surface. Most
migrations are import changes or adding `?` where caller-provided values are now
validated instead of panicking.

## Dependency features

The default feature set changed from `rpc`, `wasi-http`, `keyring`,
`file-signer`, and `tracing` to exactly `rpc`:

```toml
# RPC client with the native HTTP transport
near-kit = "0.18"

# Typed contract traits and attributes (also enables rpc)
near-kit = { version = "0.18", features = ["contracts"] }

# Offline protocol types and signing only
near-kit = { version = "0.18", default-features = false }
```

Enable `wasi-http`, `keyring`, `file-signer`, `tracing`, `mnemonic`, or `js`
explicitly when needed. BIP-39 generation and derivation APIs now require
`mnemonic`. The old `interactive-clap` and `sandbox` features no longer exist.

The `contract`, `call`, `json`, and `borsh` attributes, `Contract`,
`ContractClient`, and `Near::contract` require `contracts`, not just `rpc`.
`#[call(payable)]` was a no-op and is now rejected; use `#[call]` and attach a
deposit on the generated call builder. The `#[json]` and `#[borsh]` method
markers likewise accept no options, may appear only once, and cannot be combined
on the same method.

## Imports and namespaces

`Near`, `NearBuilder`, `Error`, `AccountId`, `CryptoHash`, `Gas`, and
`NearToken` remain at the crate root. Advanced APIs moved out of the old public
`client`, `types`, and `tokens` modules:

| Before | 0.18 |
|---|---|
| `near_kit::RpcClient`, `near_kit::RpcError`, query/response/transport types | `near_kit::rpc::{...}` |
| `near_kit::TransactionBuilder`, `FunctionCall`, `Final`, other wait markers | `near_kit::transaction::{...}` |
| `near_kit::Signer`, `SecretKey`, `InMemorySigner`, other signer/key types | `near_kit::signer::{...}` |
| `near_kit::Action`, `Transaction`, `TryIntoAccountId`, parse/protocol errors | `near_kit::protocol::{...}` |
| `near_kit::nep413` | `near_kit::standards::nep413` |
| `near_kit::FtAmount`, FT/NFT client and metadata types | `near_kit::standards::{...}` |

For example:

```rust
use near_kit::{AccountId, Error, Near, NearToken};
use near_kit::protocol::{Action, TryIntoAccountId};
use near_kit::rpc::RpcError;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::{Final, FunctionCall};
```

`AccountIdExt` and the re-exported `AccountType` wrapper were removed. Use the
classification methods provided directly by `near-account-id` when needed.

## Builders, sending, and waiting

Invalid account IDs, amounts, gas values, global-contract IDs, and serialized
arguments are retained as builder validation errors instead of panicking.
Transaction and function-call builders preserve the first error and return it
from their terminal operation (`await`, `send`, `build`, `sign`, `delegate`, or
`into_action`); construct a new call if one of those argument setters fails.
View-call argument setters remain last-one-wins, so a later successful `args`,
`args_raw`, or `args_borsh` replaces an earlier view-argument serialization
error.

`CallBuilder` now focuses on one function call. Use one of two explicit exits:

```rust
use near_kit::transaction::Final;

// Select a wait level for this one call.
let outcome = near
    .call("counter.near", "increment")
    .send()
    .wait_until::<Final>()
    .await?;

// Return to TransactionBuilder before adding another action.
near.call("counter.near", "increment")
    .finish()
    .transfer("1 NEAR")
    .send()
    .await?;
```

Directly awaiting a `CallBuilder` or `TransactionBuilder` still sends it and
uses the default `ExecutedOptimistic` wait level. Methods for transaction-wide
configuration and additional actions were removed from `CallBuilder`; insert
`.finish()` before `add_action`, `call`, `transfer`, `deploy`, `sign_with`,
`delegate`, `sign`, or similar transaction operations.

`FunctionCall` conversion is fallible. Replace `let action: Action = call.into()`
with `let action = Action::try_from(call)?` or `call.into_action()?`.
`CallBuilder::into_action()` also returns `Result` and rejects a builder that
would discard previously accumulated actions.

The redundant async conveniences were removed:

| Before | 0.18 |
|---|---|
| `near.view_with_args(id, method, &args).await?` | `near.view(id, method).args(&args).await?` |
| `near.call_with_args(id, method, &args).await?` | `near.call(id, method).args(&args).await?` |
| `near.call_with_options(id, method, &args, gas, deposit).await?` | `near.call(id, method).args(&args).gas(gas).deposit(deposit).await?` |

`Near::contract` is now fallible, so add `?`. `Near::account_id()` changed from
`&AccountId` to `Option<&AccountId>`, and `try_account_id()` was removed:

```rust
let account_id = near.account_id().ok_or(Error::NoSigner)?;
let contract = near.contract::<Counter>("counter.near")?;
```

On a signerless client, `deploy`, `deploy_from`, `publish`,
`add_full_access_key`, and `delete_key` now create a builder whose terminal
operation returns `Error::NoSigner`; they no longer panic at construction.

`IntoGlobalContractId` was replaced by fallible `TryIntoGlobalContractId`.
String errors are deferred by query/transaction builders. Code that directly
constructs `DeployGlobalContractAction` should replace
`GlobalContractDeployMode::{CodeHash, AccountId}` with
`PublishMode::{Immutable, Updatable}`; the Borsh and JSON wire values are
unchanged.

`TransactionNonceMode` was consolidated into
`near_kit::protocol::NonceMode`; its Borsh and JSON wire values are also
unchanged.

## Token helpers and amounts

The built-in `KnownToken` catalog, `USDC`, `USDT`, and `W_NEAR` constants, and
the `IntoContractId` trait were removed. Pass the contract account ID
explicitly and maintain any network-specific catalog in your application:

```rust
let token = Near::mainnet().build().ft("wrap.near")?;
```

`Near::ft` and `Near::nft` now accept `TryIntoAccountId` and return invalid IDs
immediately. Other FT/NFT caller-supplied IDs and values are surfaced through
their returned builder/result rather than panicking.

`FtAmount` is now under `near_kit::standards`. Its `checked_*` and
`saturating_*` arithmetic helpers were removed because matching only the
display symbol and decimal count cannot establish token identity. Perform
arithmetic on `raw()` values after checking the contract identity, then create
a new `FtAmount` from trusted metadata.

String unit parsing no longer truncates excess precision: NEAR supports at most
24 decimal places, and decimal Ggas/Tgas values are accepted only when exactly
representable in gas. Overflow and unsupported `FtAmount` decimal scales return
parse errors instead of overflowing internally. `NearToken` and `Gas` remain
the upstream `near-token` and `near-gas` types re-exported at the crate root.

## Signers and keys

Custom asynchronous hardware-wallet, HSM, and KMS integrations can implement
the public `near_kit::signer::SigningBackend` and pass it, together with its
claimed public key, to `SigningKey::from_backend`. Custom `Signer`
implementations must provide a side-effect-free `public_key()`; it must not
claim or rotate a key, prompt, or perform I/O.

The `KeyPair` convenience wrapper was removed. Generate or import a
`SecretKey`, then call `secret_key.public_key()` or construct an
`InMemorySigner`.

`PublicKey` and `SecretKey` representations are opaque in 0.18. Replace enum
variant construction and matching with the named factories plus `key_type()`,
`as_bytes()`, and the algorithm-specific accessors. Public-key constructors
that validate curve points are fallible, so propagate their `ParseKeyError`.
The compressed and uncompressed secp256k1 convenience constructors were
removed; normalize to the 64-byte NEAR representation and use
`PublicKey::secp256k1_from_bytes(bytes)?`.

ML-DSA-65 `SecretKey` now accepts and stores only the safe 32-byte FIPS-204
seed. The expanded-key constructors and conversions returning `SecretKey` were
removed. Use `to_ml_dsa65_expanded_bytes()` for one-way export to tooling that
requires the 4032-byte representation. Parsing or deserializing an external
4032-byte `ml-dsa-65:` string now returns `ParseKeyError::InvalidLength` rather
than importing it. `PublicKey` and `Signature` JSON and Borsh wire encodings
remain compatible.

`InMemorySigner::implicit(secret_key)` is now fallible; use
`InMemorySigner::implicit(secret_key)?`. Non-Ed25519 keys return
`SignerError::ImplicitAccountRequiresEd25519`. `generate_implicit()` remains
infallible and generates Ed25519.

### secp256k1 signing (0.18.3)

Up to 0.18.2, `SecretKey::sign` and `Signature::verify` ran SHA-256 over their
input for secp256k1 keys. Callers already pass a 32-byte digest (transaction,
delegate-action, or NEP-413 hash), so the result was a signature over
`sha256(digest)` that nearcore rejects. From 0.18.3 the 32-byte digest is
signed and verified directly, as nearcore does:

- Transactions, delegate actions and NEP-413 messages need no changes; they
  now verify on-chain.
- Signing anything other than 32 bytes with a secp256k1 key panics in
  `SecretKey::sign` and returns `SignerError::SigningFailed` from
  `SigningKey::sign`. Hash arbitrary payloads yourself (e.g. SHA-256) and sign
  the digest.
- `Signature::verify` returns `false` for non-32-byte secp256k1 input.
  Secp256k1 signatures produced by 0.18.2 or earlier do not verify against the
  original digest (they never verified on-chain either); re-sign them.

## RPC action conversion and errors

RPC deploy action views contain only a code hash, not WASM bytes. Converting
`ActionView::{DeployContract, DeployGlobalContract,
DeployGlobalContractByAccountId}` with `Action::try_from` now returns
`ActionViewConversionError::DeployCodeUnavailable { code_hash }` instead of
manufacturing an invalid deploy action whose `code` was the 32 hash bytes. Use
`ActionView::deploy_code_hash()` to inspect the hash and fetch the real code
separately when reconstruction is required.

The unused `Error::NoSignerAccount` variant was removed. Match
`Error::NoSigner` for signerless operations. Builder validation may move an
error from method construction to the terminal operation as described above.

## Docker sandbox

Docker lifecycle support moved from the `near-kit` `sandbox` feature into the
publishable `near-kit-sandbox` companion crate:

```toml
[dev-dependencies]
near-kit = "0.18"
near-kit-sandbox = "0.18"
```

Import `near_kit_sandbox::{Sandbox, SandboxConfig, SandboxError}`. Replace the
old near-kit sandbox feature/module with `SandboxConfig::fresh().await?` for an
isolated container or `SandboxConfig::shared().await?` for the process-wide
instance. `sandbox.client()` and `Near::sandbox(&sandbox)` now return a ready
`Near` directly; do not append `.build()`.

## WASI without `wasi:http`

On `wasm32-wasip2`, `wasi-http` opts into the built-in host transport. A host
that does not provide `wasi:http` should enable `rpc` without `wasi-http` and
install a custom `RpcTransport` with `NearBuilder::transport`.

Building a `Near` without either transport still succeeds. Its RPC operations
return a non-retryable `RpcError::Network` (wrapped in `Error::Rpc` by the
high-level client) rather than panicking. Use `default-features = false` for a
fully offline binary that does not need `Near` or RPC builders.
