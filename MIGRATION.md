# Migrating to near-kit 0.21

0.21 is a clean break. It shrinks near-kit to plain values plus two verbs,
and there are no deprecated aliases to migrate through. Most of the work is
mechanical:

- **Reads and writes are two types.** `Near` only reads.
  `near.with_signer(signer)` returns a `SigningNear`, which reads and sends.
- **Transactions are a receiver plus `Action`s.**
  `signing.transaction(receiver).add_action(Action::...)`, then `.await`.
  The per-action builder helpers and the `Near::transfer`/`call`/`deploy`
  shortcuts are gone.
- **Arguments are typed.** Pass an `AccountId` (`account!("x.near")` for
  literals), a `NearToken` and a `Gas`. Strings no longer convert implicitly.
- **One way to pick a block: `.at(..)`.** It takes a height, a hash, a
  `Finality` or a `SyncCheckpoint`.
- **Sends wait for `Final` by default** (was `ExecutedOptimistic`).
- **Two traits, `Viewer` and `TxSender`,** are what typed contract clients
  and the FT/NFT helpers run on. `Near` is a `Viewer`; `SigningNear` is both.
  Your own types can be either.
- **Client-free code returns its own error types** instead of
  `near_kit::Error`. They all convert into `Error` with `?`.

The protocol types also moved into the new `near-kit-primitives` crate.
near-kit re-exports them at their old `near_kit::` paths, so imports keep
working, except for the moves and renames listed under
[Renamed items](#renamed-items). Depend on `near-kit-primitives` directly only if you want the types
without the RPC client.

## Where did this method go?

The compiler reports a removed method as ``no method named `x` found for
struct `SigningNear<W>` `` with no hint where it went. Look `x` up here; the
sections below have the details. `near` is a `Near`, `signing` a `SigningNear`,
and `block` anything that converts into a `BlockReference`, like
`Finality::Final`.

| 0.20 method | 0.21 |
|---|---|
| `access_keys(id)` | `near.rpc().view_access_key_list(&id, block)` |
| `account(id)` | `near.rpc().view_account(&id, block)` |
| `account_exists(id)`, `balance(id)`, `chain_id()`, `tx_status(..)`, `view(id, m)` | On `Near`; from a `SigningNear`, `signing.near().balance(id)` and so on |
| `account_id()`, `signer()` | `signing.account_id()`, `signing.signer()` |
| `add_full_access_key(pk)`, `delete_key(pk)`, `deploy(wasm)`, `deploy_from(..)`, `publish(..)` | `signing.transaction(signing.account_id()).add_action(Action::add_full_access_key(pk))` and so on |
| `call(id, m)` | `signing.transaction(id).add_action(FunctionCall::new(m))` |
| `contract::<T>(id)` | Unchanged on both `Near` and `SigningNear` |
| `contract_code(id)` | `near.rpc().view_code(&id, block)` |
| `credentials(key, id)` (builder), `signer(s)` (builder) | `Near::testnet().build().with_signer(InMemorySigner::new(id, key.parse()?))` |
| `delegate(..)`, `sign()` (on a transaction builder) | `signing.sign_delegate(rx, actions, opts)`, `signing.sign(rx, actions)` |
| `from_env()` | `SigningNear::from_env()` |
| `ft(id)`, `nft(id)` | `near.contract::<FungibleToken>(id)`, `near.contract::<NonFungibleToken>(id)` (`near_kit::standards`) |
| `global_contract(id)` | `near.rpc().view_global_contract_code(id, block)` |
| `into_action()` (on a call) | `FunctionCall::from(call).into_action()?` |
| `max_nonce_retries(n)` | `signing.clone().max_nonce_retries(n)`, or `NearBuilder::max_nonce_retries(n)` before `build()` |
| `public_key()` | `signing.signer().public_key()` |
| `rpc()` | `near.rpc()`, or `signing.near().rpc()` |
| `rpc_url()` | `near.rpc().url()` |
| `sandbox(&sandbox)` | `sandbox.root_account()` |
| `send(&signed)` | `near.send_signed(&signed)` |
| `sign_message(params)` | `signing.signer().key().sign_nep413(signing.account_id(), &params)` |
| `state_init(si, deposit)` | `signing.transaction(si.derive_account_id()).add_action(Action::state_init(si, deposit))` |
| `transaction(rx)` | `signing.transaction(rx)` |
| `transfer(id, amount)` | `signing.transaction(id).add_action(Action::transfer(amount))` |
| `transfer_with_memo(to, amount, memo)` (token) | `transfer(to, amount, Some(memo))` |
| `validators()` | `near.rpc().validators(None)` |
| `wait_until::<W>()` (on a send) | `signing.wait_until::<W>()`, before `transaction(..)` |
| `with_signer(s)` (on a signing client) | `signing.near().with_signer(s)` |

## Clients and signers

| 0.20 | 0.21 |
|---|---|
| `Near::testnet().credentials(key, id)?.build()` | `Near::testnet().build().with_signer(InMemorySigner::new(id, key.parse()?))` |
| `NearBuilder::signer(s)` | `near.with_signer(s)` after `build()` |
| `Near::from_env()` | `SigningNear::from_env()`. It now fails unless both `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY` are set; build a `Near` for read-only use. |
| `near.with_signer(s) -> Near` | `near.with_signer(s) -> SigningNear` |
| `near.account_id()` / `try_account_id()` | `signing.account_id()`. A `Near` has no account. |
| `near.signer()` / `public_key()` | `signing.signer()` (an `&Arc<dyn Signer>`), `signing.signer().public_key()` |
| `near.sign_message(params)` | `signing.signer().key().sign_nep413(signing.account_id(), &params).await?` (or the same on any `Signer`; import `near_kit::signer::Signer`). There is no `SigningNear::sign_message`. |
| `near.rpc_url()` | `near.rpc().url()` |
| `Near::from_rpc(rpc, chain_id)` | `Near::custom(url, chain_id).transport(t).retry_config(c).build()` |
| `NearBuilder::http_client(client)` | `.transport(ReqwestTransport::with_client(client))` |
| `RpcClient::with_retry_config(url, c)` | `RpcClient::with_transport_and_retry_config(url, Arc::new(ReqwestTransport::default()), c)`, or `Near::custom(url, id).retry_config(c)` |
| `rpc.genesis_config()` | `rpc.request::<_, serde_json::Value>("genesis_config", json!([]))` |
| `rpc.call(method, params)` | `rpc.request(method, params)` |
| `Error::NoSigner` | Removed: a send without a signer is a compile error. |
| `Near::sandbox(&sandbox)`, `rpc::SandboxNetwork` | `sandbox.root_account()` |
| `InMemorySigner::new(id, "ed25519:...")?` | `InMemorySigner::new(id, "ed25519:...".parse()?)`. `new` takes a `SecretKey` and can't fail. |
| `InMemorySigner::from_secret_key(id, sk)?`, `InMemorySigner::from_parts(id, sk)` | `InMemorySigner::new(id, sk)` |
| `InMemorySigner::generate_implicit()` | `InMemorySigner::implicit(SecretKey::generate_ed25519())?` |
| `InMemorySigner::from_seed_phrase(id, phrase)` / `from_seed_phrase_with_path(id, phrase, path)` | `InMemorySigner::new(id, SecretKey::from_seed_phrase(KeyType::Ed25519, phrase, None, None)?)` (`Some(path)` for a path) |
| `SecretKey::{from_seed_phrase, from_seed_phrase_with_path, from_seed_phrase_with_path_and_passphrase}` and the `ml_dsa65_` versions | `SecretKey::from_seed_phrase(key_type, phrase, hd_path, passphrase)`: `KeyType::Ed25519` or `KeyType::MlDsa65`; `None` is the default path / no passphrase |
| `SecretKey::{generate_with_seed_phrase, generate_with_seed_phrase_words, generate_with_seed_phrase_custom}` and the `ml_dsa65_` versions, `signer::generate_seed_phrase` | `SecretKey::generate_with_seed_phrase(key_type, word_count)`; `None` words is 12 (Ed25519) or 24 (ML-DSA-65). For another path or a passphrase, derive again from the returned phrase. |
| `signer::{DEFAULT_HD_PATH, DEFAULT_WORD_COUNT, DEFAULT_ML_DSA_65_WORD_COUNT}` | Removed; pass `None` for the default |
| `RotatingSigner::from_key_strings(id, &keys)` | `RotatingSigner::new(id, keys.iter().map(|k| k.parse()).collect::<Result<_, _>>()?)` |
| `RotatingSigner::key_count()` | `public_keys().len()` |
| `signer::EnvSigner::new()` / `EnvSigner::from_env_vars(a, k)` | `signer::from_env()` / `signer::from_env_vars(a, k)`, returning an `InMemorySigner` |
| `signer::FileSigner::new(network, id)` / `FileSigner::from_file(path, id)` | `signer::from_credentials(network, id)` / `signer::from_credentials_file(path, id)` |
| `signer::KeyringSigner::new(network, id, "ed25519:...")` | `signer::from_keyring(network, id, &public_key)` (a parsed `PublicKey`) |
| `.into_inner()` on those signers | Not needed: the loaders return the `InMemorySigner` |
| `signer.public_key()` on an `InMemorySigner` (inherent, `&PublicKey`) | The `Signer` trait method (`PublicKey`); import `near_kit::signer::Signer` |

`SigningNear` has no `Deref` to `Near`; read through `signing.near()`. Typed
contract views and token views work on a `SigningNear` directly.

```rust
// 0.20
let near = Near::testnet().credentials("ed25519:...", "me.testnet")?.build();
let balance = near.balance("me.testnet").await?;

// 0.21
let me = Near::testnet()
    .build()
    .with_signer(InMemorySigner::new(account!("me.testnet"), "ed25519:...".parse()?));
let balance = me.near().balance(me.account_id()).await?;
```

## Transactions

| 0.20 | 0.21 |
|---|---|
| `near.transaction(rx)` (a `TransactionBuilder`) | `signing.transaction(rx)` (a `Tx`) |
| `.transfer(amount)`, `.create_account()`, `.deploy(wasm)`, `.add_full_access_key(pk)`, `.add_function_call_key(..)`, `.delete_key(pk)`, `.delete_account(b)`, `.stake(..)`, `.publish(..)`, `.deploy_from(..)`, `.state_init(..)`, `.universal_state_init(..)` | `.add_action(Action::transfer(amount))` and so on: one `Action::*` constructor per helper. `deploy_from` becomes `Action::deploy_from(id)` (a publisher `AccountId`) or `Action::deploy_from(hash)` (a code `CryptoHash`). |
| `.call("m").args(a).gas(g).deposit(d)` … `.finish()` | `.add_action(FunctionCall::new("m").args(a).gas(g).deposit(d))` |
| `near.call(id, "m")` | `signing.transaction(id).add_action(FunctionCall::new("m"))` |
| `near.transfer(id, amount)` | `signing.transaction(id).add_action(Action::transfer(amount))` |
| `near.deploy(wasm)`, `publish`, `deploy_from`, `add_full_access_key`, `delete_key` | `signing.transaction(signing.account_id()).add_action(Action::...)` |
| `near.state_init(si, deposit)` / `universal_state_init(..)` | `signing.transaction(si.derive_account_id()).add_action(Action::state_init(si, deposit))`. The receiver is no longer filled in for you; a state init sent to any other receiver fails at `.await` with `Error::InvalidTransaction`, before signing. |
| `.add_actions(vec)` | Call `add_action` in a loop. |
| `.send()`, `.send().wait_until::<W>()` | `.await` sends; set the wait level on the client (see below). |
| `.signed_delegate_action(signed)` (relayer) | `relayer.transaction(signed.sender_id()).add_action(Action::delegate(signed))`. The receiver must be the delegate's sender; any other receiver fails at `.await` with `Error::InvalidTransaction`, before signing. |
| `.sign()` / `.delegate(opts)` | `signing.sign(rx, actions)` / `signing.sign_delegate(rx, actions, opts)` |
| `DelegateResult` (`.signed_delegate_action`, `.payload`, `.sender_id()`) | `sign_delegate` returns the `SignedDelegateAction` itself: use it directly, `.to_base64()` for the payload |
| `DelegateOptions::with_offset(n)` / `with_max_height(h)` | `DelegateOptions { block_height_offset: Some(n), ..Default::default() }` / `max_block_height: Some(h)` |
| `.sign_with(other)` | `signing.near().with_signer(other).transaction(..)` |
| `.max_nonce_retries(n)` | `signing.clone().max_nonce_retries(n)` for one sender, or `NearBuilder::max_nonce_retries(n)` for every `SigningNear` made from that `Near` |
| `near.max_nonce_retries(n)` | `NearBuilder::max_nonce_retries(n)` before `build()` |
| `.build()`, `.build_offline(..)`, `.sign_offline(..)`, `.into_parts()` | `protocol::Transaction::new(..).sign(&key)` |
| `Action::deploy_from_hash(hash)` / `Action::deploy_from_account(id)` | `Action::deploy_from(hash)` / `Action::deploy_from(id)` |
| `Transaction::complete(sig)`, `TransactionV1::complete(sig)`, `DelegateAction::sign(sig)`, `VersionedDelegateActionPayload::sign(sig)` | `into_signed(sig)` |
| `near.send(&signed)` | `near.send_signed(&signed)` |
| `CallBuilder`, `TransactionBuilder`, `TransactionSend` | `Tx` |

`Tx::add_action` takes anything that converts into an `Action`, including a
`FunctionCall`. An argument-encoding error is returned from `.await`, as
before.

```rust
// 0.20
near.transaction("sub.me.testnet")
    .create_account()
    .transfer(NearToken::from_near(5))
    .add_full_access_key(key)
    .deploy(wasm)
    .call("new")
        .args(serde_json::json!({ "owner_id": "me.testnet" }))
    .send()
    .await?;

// 0.21
me.transaction(account!("sub.me.testnet"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(5)))
    .add_action(Action::add_full_access_key(key))
    .add_action(Action::deploy_contract(wasm))
    .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner_id": "me.testnet" })))
    .await?;
```

### Wait levels

The wait level is now part of the signing client's type, set once with
`.wait_until::<W>()`. `SigningNear` (no parameter) means
`SigningNear<Final>`. **The default changed from `ExecutedOptimistic` to
`Final`.** This is not a compile error, and it makes sends slower but means a
default (final) read sees the write. To keep the 0.20 behaviour, use
`.wait_until::<ExecutedOptimistic>()`.

```rust
// 0.20
let response = near
    .transfer("bob.testnet", NearToken::from_near(1))
    .send()
    .wait_until::<Included>()
    .await?;

// 0.21
let response = me
    .wait_until::<Included>()
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?;
```

### Extension traits over a signing client

Helper traits implemented for `Near` move to `SigningNear`:

```rust
// 0.20
trait Wrap {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error>;
}
impl Wrap for Near {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error> {
        self.call("wrap.testnet", "near_deposit").deposit(amount).await?;
        Ok(())
    }
}

// 0.21
trait Wrap {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error>;
}
impl Wrap for SigningNear {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error> {
        self.transaction(account!("wrap.testnet"))
            .add_action(FunctionCall::new("near_deposit").deposit(amount))
            .await?;
        Ok(())
    }
}
```

A helper that only sends can instead be written once for every `TxSender`, so
it also runs on test doubles and other senders:

```rust
trait WrapAny: TxSender {
    async fn wrap(&self, amount: NearToken) -> Result<Self::Output, Self::SendError> {
        let deposit = Action::function_call("near_deposit", "{}", Gas::from_tgas(10), amount);
        self.send(account!("wrap.testnet"), vec![deposit]).await
    }
}
impl<S: TxSender> WrapAny for S {}
```

## Reads

| 0.20 | 0.21 |
|---|---|
| `.at_block(h)`, `.at_block_hash(hash)`, `.finality(f)`, `.block_reference(r)` | `.at(h)`, `.at(hash)`, `.at(f)`, `.at(r)` |
| `BlockReference::at_height(h)`, `at_hash`, `final_()`, `near_final()`, `optimistic()` | Pass `h`, `hash` or `Finality::Final` straight to `.at(..)` or an `RpcClient` read (they take `impl Into<BlockReference>`). Don't write `.into()` there: with a generic parameter rustc can't infer the target and reports "type annotations needed". Where a `BlockReference` value is required, use `BlockReference::from(h)` or the enum variant. |
| `BlockReference::genesis()` / `earliest_available()` | Pass `SyncCheckpoint::Genesis` / `SyncCheckpoint::EarliestAvailable` directly; `BlockReference::from(..)` where a value is required |
| `BlockReference::to_rpc_params()` | Removed. `RpcClient` builds the params. |
| `near.view::<T>(id, m)` (a `ViewCall<T>`) | `near.view::<T>(id, m)` (a `View<Near, T>`) |
| `near.view::<T>(id, m).borsh()` | `View::<_, T>::borsh(near.clone(), id, m)` |
| `near.account(id)` | `near.rpc().view_account(&id, block)` |
| `near.access_keys(id)` / `.page(..)` | `near.rpc().view_access_key_list(&id, block)` / `view_access_key_list_page(..)` |
| `near.contract_code(id)` / `.exists()` | `near.rpc().view_code(&id, block)`; for `exists`, treat `is_contract_not_deployed()` / `is_account_not_found()` as `false` |
| `near.global_contract(id)` / `.exists()` | `near.rpc().view_global_contract_code(id, block)` with a `CryptoHash`, `AccountId` or `GlobalContractId`; match `is_global_contract_not_found()` |
| `near.validators()` | `near.rpc().validators(None)` |
| `AccountQuery`, `AccessKeysQuery`, `ContractCodeQuery`, `GlobalContractQuery`, `ViewCall`, `ViewCallBorsh` | Removed (see above) |
| `rpc::{LightClientBlockView, LightClientBlockLiteView, BlockHeaderInnerLiteView}`, `rpc::{StateChangeWithCauseView, StateChangeCauseView, StateChangeValueView}` | Removed: no `RpcClient` method returned them. For `EXPERIMENTAL_changes` or light-client proofs, use `near.rpc().request::<_, serde_json::Value>(method, params)` or your own serde type. |
| `view_result.as_string()` | `view_result.text()` |
| `gas_price.as_u128()` | `gas_price.gas_price.as_yoctonear()` |

`balance`, `account_exists`, `tx_status` and `view` stay on `Near`. `block` is
anything that converts into a `BlockReference`, like `.at(..)`: a `Finality`, a
height, a block hash or a `SyncCheckpoint`. `RpcClient` reads take the account
as `&AccountIdRef`, so `&AccountId` and `account!(..)` both work. Pass
`Finality::Final` where 0.20 code read the default; `Default::default()` no
longer infers.

A view now returns `Result<T, ContractError<RpcError>>` rather than
`Result<T, Error>`. `?` into `Error` still works. Code that matched `Error`
should match `ContractError::Backend(rpc_error)` /
`ContractError::Codec(codec_error)`, or call `.map_err(Error::from)` first.

If your own error enum wraps `near_kit::Error` with `#[from]`, `?` on a typed
view or call no longer reaches it: `ContractError<RpcError>` to `Error` to
your enum is two conversions, and `?` does one. Convert first, or add the
second `From`:

```rust
// MyError has a `Near(#[from] near_kit::Error)` variant.
let count: u64 = near
    .view(account!("counter.testnet"), "get_count")
    .await
    .map_err(near_kit::Error::from)?;

// or, once, so a plain `?` works:
impl From<ContractError<RpcError>> for MyError {
    fn from(e: ContractError<RpcError>) -> Self {
        MyError::Near(e.into())
    }
}
```

```rust
// 0.20
let count: u64 = near.view("counter.testnet", "get_count").at_block(180_000_000).await?;
let read = near.view::<u64>("counter.testnet", "get_count").with_metadata().await?;
let account = near.account("alice.testnet").finality(Finality::Optimistic).await?;

// 0.21
let count: u64 = near.view(account!("counter.testnet"), "get_count").at(180_000_000).await?;
let read = near.view::<u64>(account!("counter.testnet"), "get_count").with_metadata().await?;
println!("{} at block {}", read.result, read.block_height);
let account = near
    .rpc()
    .view_account(&account!("alice.testnet"), Finality::Optimistic)
    .await?;
```

## Typed arguments

| 0.20 | 0.21 |
|---|---|
| `impl TryIntoAccountId` (`"alice.near"`, `String`) | `impl Into<AccountId>` (`AccountId`, `&AccountId`, `&AccountIdRef`). Use `account!("alice.near")` for a literal (checked at compile time) or `"alice.near".parse::<AccountId>()?` for runtime input. |
| `impl IntoNearToken` (`"5 NEAR"`) | `NearToken` (`NearToken::from_near(5)`, or `"5 NEAR".parse()?`) |
| `impl IntoGas` (`"30 Tgas"`) | `Gas` (`Gas::from_tgas(30)`, or `"30 Tgas".parse()?`) |
| `impl TryIntoGlobalContractId` | `Action::deploy_from(code)`, with an `AccountId`, a `CryptoHash` or a `GlobalContractId` |
| `protocol::{TryIntoAccountId, IntoNearToken, IntoGas, TryIntoGlobalContractId, ParseGasError}` | Removed |
| `Error::ParseAmount`, `Error::ParseGas`, `ParseAmountError::AmbiguousAmount` | Removed. So is the rest of `ParseAmountError`, with `FtAmount` (see tokens). |

This also covers the signer constructors and loaders (`InMemorySigner`,
`RotatingSigner`, `signer::from_credentials*`, `signer::from_keyring`), `FunctionCall::gas`/`deposit`, the FT/NFT
helpers, and `near-kit-sandbox`.

## Typed contracts

| 0.20 | 0.21 |
|---|---|
| `trait Contract { type Client: ContractClient }`, `ContractClient::new(near, id)` | `trait Contract { type Client<C>; fn client<C>(backend: C, id) }` |
| `CounterClient` (holds a `Near`) | `CounterClient<C = Near>`. Views need `C: Viewer + Clone`; `#[call]` methods need `C: TxSender + Clone`. |
| `near.contract::<Counter>(id).increment()` | `signing.contract::<Counter>(id).increment()`. `near.contract` gives a view-only client. |
| A call method resolves to `FinalExecutionOutcome` | It resolves to the backend's output: `W::Response` for `SigningNear<W>` (`FinalExecutionOutcome` for the default `Final`) |
| `#[call] fn m(&mut self) -> T` (return type ignored) | A compile error. Decode the outcome yourself. |
| Contract methods named `contract_id` or `client` | A compile error. Rename them. |
| `near_kit::{call, json, borsh}` attribute re-exports and the standalone `near_kit_macros::{call, json, borsh}` | Removed. `#[contract]` reads the markers itself and they need no import. |

Views return `View` and calls return `Call` (both at the crate root), so
they take `.at(..)` and `.gas(..)`/`.deposit(..)` before `.await`, the same as
`Near::view`. Their error is `ContractError<C::ViewError>` /
`ContractError<C::SendError>`. To use a different wait level, build the client
from a re-levelled sender:
`signing.wait_until::<Included>().contract::<Counter>(id)`.

```rust
// 0.20
let counter = near.contract::<Counter>("counter.testnet".parse::<AccountId>()?);
counter.increment().await?;

// 0.21
let counter = me.contract::<Counter>(account!("counter.testnet"));
counter.increment().gas(Gas::from_tgas(10)).await?;
let count = counter.get_count().at(180_000_000).await?;
```

## Your own backend

`Viewer` and `TxSender` are new. Implement one or both, and typed contract
clients, `View`/`Call` and the token helpers run on your type. Build the
view result with `ViewFunctionResult::new(bytes, block_height, block_hash)`.
On wasm targets, write `MaybeSend`, not `Send`, in any forwarding bounds.

```rust
#[derive(Clone)]
struct Gateway;

impl Viewer for Gateway {
    type ViewError = String;

    async fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        // ...call your service...
        Ok(ViewFunctionResult::new(b"42".to_vec(), 1, CryptoHash::ZERO))
    }
}

let count = Counter::client(Gateway, account!("counter.testnet")).get_count().await?;
```

Both traits are implemented for `&T`, `Box<T>` and `Arc<T>` (including
`T: ?Sized`), so `Arc<dyn MySender>` works once you implement `TxSender for
dyn MySender`.

## Fungible and non-fungible tokens

| 0.20 | 0.21 |
|---|---|
| `FungibleToken`, `NonFungibleToken` (the clients) | `FungibleTokenClient<C = Near>`, `NonFungibleTokenClient<C = Near>`. `FungibleToken` and `NonFungibleToken` are now unit `Contract` markers, like the ones `#[contract]` generates. |
| `near.ft(id)`, `near.nft(id)` | `near.contract::<FungibleToken>(id)`, `near.contract::<NonFungibleToken>(id)` (import them from `near_kit::standards`) |
| `near.ft(id).transfer(to, amount)` on a signing `Near` | `signing.contract::<FungibleToken>(id).transfer(to, amount, None)`. The client from `near.contract::<FungibleToken>(id)` is read-only. |
| `transfer_with_memo(to, amount, memo)` | `transfer(to, amount, Some(memo))` |
| `balance_of(..)` / `total_supply()` returning `FtAmount` (`.raw()`, `.decimals()`, `.symbol()`, `Display`) | They return `u128` in the token's smallest unit and no longer read metadata first. Drop `.raw()`; for display, use `metadata().await?.decimals` and `.symbol`. |
| `FtAmount`, `FtAmount::parse`, `protocol::ParseAmountError` | Removed |
| `.with_signer(s)` on a token client | `near.with_signer(s).contract::<FungibleToken>(id)` |
| Token calls return a `CallBuilder` | They return a `Call<C>`: `.gas(..)`, `.deposit(..)`, `.await` |
| `call_builder.into_action()` | `FunctionCall::from(call).into_action()?`, or pass `FunctionCall::from(call)` to `add_action` |
| Token views return `Error` | They return `ContractError<C::ViewError>` (`?` still converts) |

`FungibleToken::client(backend, id)` (the `Contract` trait) builds a client
over any backend; clones of a client share its metadata cache. A
non-numeric `U128`/`U64` string from a token contract is now
`ContractError::Codec`, not an RPC error. To batch a token or contract call
with other actions, convert it: `FunctionCall::from(call)` keeps the method,
arguments, gas and deposit, and `tx.add_action(..)` accepts it.

```rust
// 0.20
near.ft("wrap.near".parse::<AccountId>()?)
    .transfer_with_memo("bob.near", 1_000_u128, "rent")
    .await?;

// 0.21
use near_kit::standards::FungibleToken;

me.contract::<FungibleToken>(account!("wrap.near"))
    .transfer(account!("bob.near"), 1_000_u128, Some("rent".to_owned()))
    .await?;
```

## Errors in client-free code

Code that runs without an RPC client returns its own error type instead of
`near_kit::Error`:

| API | Now returns |
|---|---|
| `SignedTransaction(V1)::from_bytes` / `from_base64` | `DecodeTransactionError` |
| `FinalExecutionOutcome::result` / `json` | `OutcomeError` |
| `FunctionCall::into_action`, `TryFrom<FunctionCall>` | `CodecError` |
| `InMemorySigner::implicit`, `SecretKey::from_seed_phrase` / `generate_with_seed_phrase`, `RotatingSigner::new` / `from_signers` | `SignerError` |

Each converts into `near_kit::Error` with `?`. `DecodeTransactionError` now
becomes `Error::Decode`, and an `OutcomeError` other than `InvalidTx` or `Json`
becomes `Error::Outcome`; both used to be an `Error::InvalidTransaction`
string, which is now only for local checks before sending (no actions, the
wrong receiver, a nested delegate). The rest land in the same variant, with
the same message, as before. Two patterns need a change:

- **Passing a variant constructor to `map_err`.** Convert first:
  `.map_err(|e| MyError::Signer(e.into()))?`.
- **Matching the result directly.** Match the new type's variants (for example
  `OutcomeError::InvalidTx` instead of `Error::InvalidTx`), or call
  `.map_err(near_kit::Error::from)` first.

`SignerError` is `#[non_exhaustive]` and gained `InvalidKey`, `NoKeys` and
`MixedAccounts`. `WaitLevel::convert` is `#[doc(hidden)]`, takes an
`RpcTransactionResponse` and returns `OutcomeError`.

The new view and call values have one error type, `ContractError<E>`:
`Codec(CodecError)` for argument or result encoding, or `Backend(E)` for the
backend's error. It implements `std::error::Error` whenever `E` is `Debug +
Display`, so `?` into `anyhow` works, and `From<ContractError<E>> for Error`
covers near-kit's backends.

These had no producer or only an internal use, and are gone:

| 0.20 | 0.21 |
|---|---|
| `Error::DelegateDecode` | Removed: nothing returned it. `SignedDelegateAction::from_base64` returns `DecodeDelegateError` itself. |
| `Error::is_invalid_tx()` | `matches!(e, Error::InvalidTx(_))` |
| `From<Infallible> for Error` | Removed |
| `RpcError::network(..)`, `RpcError::invalid_transaction(..)` | Crate-private. Build the variant (`RpcError::Network { .. }`) in a custom transport. |
| `RpcClient::DEFAULT_VIEW_STATE_PAGE_SIZE`, `DEFAULT_VIEW_ACCESS_KEYS_PAGE_SIZE` | Crate-private (10,000 and 100) |

## Sandbox

| 0.20 | 0.21 |
|---|---|
| `sandbox.client() -> Near` | `sandbox.root_account() -> SigningNear` for the root account |
| `Near::sandbox(&sandbox)` | `sandbox.root_account()` |
| `SandboxBuilder::root_account("x")`, `set_balance("x", ..)` | Take `impl Into<AccountId>`: `account!("x")` |
| `SANDBOX_ROOT_ACCOUNT: &str` | `&AccountIdRef` |
| `SandboxError::{InvalidRootAccount, InvalidAccount}` | Removed (account IDs are typed) |
| `SandboxConfig::shared()` / `fresh()` / `builder()` | `Sandbox::builder().shared()` / `.fresh()` / `Sandbox::builder()` |
| `sandbox.rpc_url()` | `sandbox.root_account().near().rpc().url()` |
| `sandbox.root_account_id()` | `sandbox.root_account().account_id()` |
| `sandbox.chain_id()` | `sandbox.root_account().near().chain_id()` |
| `sandbox.root_signer()` | `sandbox.root_account().signer().clone()` |
| `rpc.sandbox_fast_forward(n)` / `rpc.sandbox_patch_state(records)` | `sandbox.fast_forward(n)` / `sandbox.patch_state(records)` (still sends the patch twice; it no longer sleeps 100 ms afterwards, so poll for the state you wrote, or use `sandbox.set_balance(..)`, which does) |

```rust
// 0.20
let root = SandboxConfig::shared().await?.client();
root.transaction("alice.sandbox")
    .create_account()
    .transfer(NearToken::from_near(10))
    .add_full_access_key(key)
    .await?;

// 0.21
let root = Sandbox::builder().shared().await?.root_account();
root.transaction(account!("alice.sandbox"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(10)))
    .add_action(Action::add_full_access_key(key))
    .await?;
```

## Renamed items

Items that keep their role under a new name or path:

| 0.20 | 0.21 |
|---|---|
| `protocol::AccountIdRef` | `AccountIdRef` (crate root) |
| `RpcError::UnknownBlock`, `UnknownChunk`, `UnknownEpoch`, `UnknownReceipt` | `RpcError::BlockNotFound`, `ChunkNotFound`, `EpochNotFound`, `ReceiptNotFound` |
| `RpcError::RequestTimeout { .. }` | `RpcError::Timeout { .. }` |
| `RpcError::Timeout(u32)` | Removed: nothing returned it |
| `RpcError::InvalidTransaction { message, details }` | `RpcError::InvalidTxUnparsed { message, details }` |
| `protocol::DelegateDecodeError` | `protocol::DecodeDelegateError` |
| `rpc::RawTransactionResponse` (hidden) | `rpc::RpcTransactionResponse`, documented: what `RpcClient::send_tx` and `tx_status` return |
| `standards::nep413::serialize_message(params)` | `nep413::hash_message(params)` |
| `PublicKeyHandle::full_pubkey()` | `PublicKeyHandle::as_full()` |
| `protocol::MAX_NONCES_FOR_GAS_KEY` | `AccessKeyPermission::MAX_NONCES_FOR_GAS_KEY` |
| `StateInitExt` (`StateInit::by_hash(hash, data)`, `StateInit::by_publisher(id, data)`) | Removed. `StateInitV1::code(hash_or_id).with_data_entry(k, v).into()`, or `StateInit::V1(StateInitV1 { code: hash_or_id.into(), data })` |

## Macros crate

`near-kit-macros` changed incompatibly with this release (`Client`, the
reserved method names, the removed standalone markers). Depend on near-kit
with the `contracts` feature, or on near-kit-primitives with `contracts`,
rather than on `near-kit-macros` directly.

# Migrating to near-kit 0.19

0.19 walks back a few 0.18 changes that made downstream code noisier. Most of
the migration is deleting `.unwrap()` and `?` that 0.18 required.

- **`PublicKey` is a public enum again**: `Ed25519([u8; 32])`,
  `Secp256k1([u8; 64])`, `MlDsa65(Box<[u8; 1952]>)`. Match on it directly
  instead of `key_type()` plus `as_*_bytes().unwrap()`. The 0.18 constructors
  and accessors still exist.
- **Public keys decode like nearcore.** `FromStr`, Borsh and serde check only
  the type tag and length, so off-curve ed25519/secp256k1 keys now decode
  (including in `PublicKeyHandle` and access-key list views). To reject them,
  call `PublicKey::validate()` or use the checked
  `ed25519_from_bytes`/`secp256k1_from_bytes` constructors.
- **`Near::account_id()` returns `&AccountId`** and panics if no signer is
  configured. Use `try_account_id()` for `Option<&AccountId>`:

  ```rust
  let me = near.account_id();                 // was near.account_id().unwrap()
  let maybe_me = near.try_account_id();       // was near.account_id()
  ```

- **`Near::contract`, `Near::ft` and `Near::nft` are infallible** and take
  `impl Into<AccountId>` (`AccountId`, `&AccountId`, `&AccountIdRef`). Drop the
  `?`; parse strings first or use a `const` literal:

  ```rust
  use near_kit::protocol::AccountIdRef;
  const WRAP: &AccountIdRef = AccountIdRef::new_or_panic("wrap.near");

  let counter = near.contract::<Counter>(near.account_id());
  let token = near.ft(WRAP);
  let nft = near.nft("nft.near".parse::<AccountId>()?);
  ```

- **`contracts` no longer implies `rpc`.** With
  `default-features = false, features = ["contracts"]`, `#[near_kit::contract]`
  generates the offline `FunctionCall` constructors (e.g. `Counter::add(..)`)
  for composing transactions anywhere, including `wasm32` without a network
  stack. The `<Name>Client`, `Contract`, `ContractClient` and `Near::contract`
  need `rpc` too, so a crate that uses them with `default-features = false`
  must enable both: `features = ["rpc", "contracts"]`. The default feature set
  already includes `rpc`.
- `near-kit-macros` must match near-kit: its expansion relies on a support
  shim that only near-kit 0.19+ provides. Depend on `near-kit` with the
  `contracts` feature rather than on `near-kit-macros` directly.

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
