# Typed contracts and tokens

Read this when you call the same contract from many places and want the
compiler to check method names and argument types, or when you work with
fungible (NEP-141) or non-fungible (NEP-171) tokens. `#[near_kit::contract]`
turns a Rust trait that describes a contract's interface into a client, and
near-kit ships two such clients for the token standards. The macro needs
the `contracts` feature; the token clients need only `rpc`, which is on by
default.

## Declaring a contract

```rust
use serde::{Deserialize, Serialize};

#[near_kit::contract]
pub trait Guestbook {
    /// The newest messages.
    fn get_messages(&self, args: GetMessages) -> Vec<Message>;

    fn total_messages(&self) -> u64;

    #[call]
    fn add_message(&mut self, args: AddMessage);
}

/// The contract takes `U64` numbers, which are strings in JSON.
#[derive(Serialize)]
pub struct GetMessages {
    pub from_index: String,
    pub limit: String,
}

#[derive(Serialize)]
pub struct AddMessage {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub premium: bool,
    pub sender: String,
    pub text: String,
}
```

A method that takes `&self` is a view; one that takes `&mut self` and is
marked `#[call]` is a change method. For `trait Guestbook` the macro
generates:

- a unit struct `Guestbook`, with a `FunctionCall` constructor for each
  `#[call]` method (`Guestbook::add_message(args)`);
- `GuestbookClient<C>`, a client over a backend `C` (`C` defaults to
  `Near`), with one method per trait method and `contract_id()`;
- `impl Contract for Guestbook`, which is how
  [`Near::contract`](crate::Near::contract) and
  [`SigningNear::contract`](crate::SigningNear::contract) build the client.

## Calling it

```rust,no_run
# use serde::{Deserialize, Serialize};
# #[near_kit::contract]
# pub trait Guestbook {
#     fn get_messages(&self, args: GetMessages) -> Vec<Message>;
#     fn total_messages(&self) -> u64;
#     #[call]
#     fn add_message(&mut self, args: AddMessage);
# }
# #[derive(Serialize)]
# pub struct GetMessages { pub from_index: String, pub limit: String }
# #[derive(Serialize)]
# pub struct AddMessage { pub text: String }
# #[derive(Debug, Deserialize)]
# pub struct Message { pub premium: bool, pub sender: String, pub text: String }
use near_kit::rpc::Finality;
use near_kit::*;

# async fn example(near: &Near, alice: &SigningNear) -> Result<(), Error> {
let id = account!("guestbook.testnet");

// Read-only: a client over a Near has the views.
let guestbook = near.contract::<Guestbook>(&id);
let total = guestbook.total_messages().await?;
let latest = guestbook
    .get_messages(GetMessages { from_index: "0".into(), limit: "10".into() })
    .at(Finality::Optimistic)
    .await?;

// Over a SigningNear it has the calls too, sent as alice.
let guestbook = alice.contract::<Guestbook>(&id);
guestbook
    .add_message(AddMessage { text: "hello".into() })
    .gas(Gas::from_tgas(10))
    .deposit(NearToken::from_millinear(10))
    .await?
    .result()?;
# Ok(())
# }
```

Views return a [`View`](crate::View), the same type as
[`Near::view`](crate::Near::view): `.at(..)` picks the block (final by
default), `.with_metadata()` also returns the block height and hash.
Calls return a [`Call`](crate::Call): `.gas(..)` (30 Tgas by default) and
`.deposit(..)` (zero by default), then `.await`. A call resolves to the
backend's [`Output`](crate::TxSender::Output): for a `SigningNear` at the
default level that is a [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome),
which can hold a failed action, as with any send (see the
[errors chapter](crate::guide::errors)). To decode a call's return value,
use `outcome.json::<T>()`.

A client over a `Near` has no call methods: calling one is a compile error
saying the backend "cannot send transactions". For another wait level, build
the client from a re-levelled sender:
`alice.wait_until::<Included>().contract::<Guestbook>(&id)`.

## Batching typed calls

The generated constructors return a
[`FunctionCall`](crate::transaction::FunctionCall) without sending, so a
typed call can share a transaction with other actions:

```rust,no_run
# use serde::Serialize;
# #[near_kit::contract]
# pub trait Guestbook {
#     #[call]
#     fn add_message(&mut self, args: AddMessage);
# }
# #[derive(Serialize)]
# pub struct AddMessage { pub text: String }
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
alice
    .transaction(account!("guestbook.testnet"))
    .add_action(Guestbook::add_message(AddMessage { text: "one".into() }).gas(Gas::from_tgas(10)))
    .add_action(Guestbook::add_message(AddMessage { text: "two".into() }).gas(Gas::from_tgas(10)))
    .await?.result()?;
# Ok(())
# }
```

`FunctionCall::from(call)` does the same for a `Call` you already have,
keeping the gas and deposit set on it.

## Rules for the trait

The macro rejects, with a compile error, what it can't generate correctly:

- **Receivers.** Every method takes `&self` (a view) or `&mut self` with
  `#[call]`. `#[call]` on `&self`, or `&mut self` without it, is an error.
- **Arguments.** At most one, bound to a plain name (`args: Args`, or
  `mut args`). It becomes the method's whole argument object: a JSON struct's
  fields are the contract's parameter names. For several parameters, use a
  struct. `impl Trait` and concrete types work; generic methods, lifetime
  parameters, `where` clauses and patterns such as `_` or `(a, b)` don't.
  A method without an argument sends `{}` in JSON, and nothing in Borsh.
- **Return types.** A view's return type is what its result decodes into
  (`()` when there is none; an empty result decodes as `null`, so `()` and
  `Option<_>` work). A `#[call]` can't declare one, since it resolves to the
  sender's output; decode the outcome instead.
- **Not supported:** `async fn`, default method bodies, generic or
  `unsafe` traits, supertraits, and items other than methods.
- **Reserved names.** `client`, `contract_id` and `clone` collide with the
  generated client and are rejected; rename the method.

Two attributes carry through:

- **Raw identifiers** name the method without `r#`: `fn r#type(&self)`
  calls the contract method `type`.
- **`#[cfg]` and `#[cfg_attr]`** on a method apply to everything generated
  for it, the client method and the constructor. Doc comments are copied
  onto both.

```rust
#[near_kit::contract]
pub trait Registry {
    /// Calls the contract method `type`.
    fn r#type(&self) -> String;

    /// Only generated for native builds.
    #[cfg(not(target_arch = "wasm32"))]
    #[call]
    fn reindex(&mut self);
}
```

### Borsh

Arguments and results are JSON by default. `#[near_kit::contract(borsh)]`
makes Borsh the default for the whole trait, and `#[borsh]` or `#[json]`
on a method overrides it for that method:

```rust
use borsh::{BorshDeserialize, BorshSerialize};

#[near_kit::contract(borsh)]
pub trait Counter {
    fn get(&self) -> u64;

    #[call]
    fn set(&mut self, args: Set);

    #[json]
    fn metadata(&self) -> serde_json::Value;
}

#[derive(BorshSerialize)]
pub struct Set {
    pub value: u64,
}
```

## Clients over other backends

The client is generic over its backend: views need
[`Viewer`](crate::Viewer) + `Clone`, calls [`TxSender`](crate::TxSender) +
`Clone`. Build one over any backend with
[`Contract::client`](crate::Contract::client), as `near.contract` does:

```rust
# #[near_kit::contract]
# pub trait Guestbook { fn total_messages(&self) -> u64; }
use near_kit::{Contract, Near, account};

let guestbook = Guestbook::client(Near::testnet().build(), account!("guestbook.testnet"));
assert_eq!(guestbook.contract_id(), "guestbook.testnet");
```

A backend that isn't `Clone` works through `&T` or `Arc<T>`. The
[testing chapter](crate::guide::testing) runs generated clients over mock
backends, and the [platforms chapter](crate::guide::platforms) uses the
macro inside a smart contract through near-kit-primitives.

## Fungible tokens

[`FungibleToken`](crate::standards::FungibleToken) is a marker like the
ones the macro generates. `near.contract::<FungibleToken>(id)` gives a
read-only [`FungibleTokenClient`](crate::standards::FungibleTokenClient),
and `alice.contract::<FungibleToken>(id)` one that also transfers.

Amounts are `u128` in the token's smallest unit, as the contract stores
them. [`metadata`](crate::standards::FungibleTokenClient::metadata) has the
`decimals` to scale by; it is fetched once per client and cached, and
clones of the client share the cache.

```rust,no_run
use near_kit::standards::FungibleToken;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let usdc = alice.contract::<FungibleToken>(account!("usdc.testnet"));

let meta = usdc.metadata().await?;
let raw = usdc.balance_of(alice.account_id()).await?;
let scale = 10u128.pow(meta.decimals.into());
println!("{}.{:0width$} {}", raw / scale, raw % scale, meta.symbol, width = meta.decimals as usize);

// A receiver must be registered (NEP-145 storage) before it can hold tokens.
let bob = account!("bob.testnet");
if !usdc.is_registered(&bob).await? {
    let bounds = usdc.storage_balance_bounds().await?;
    usdc.storage_deposit(&bob, bounds.min).await?.result()?;
}

// 2.5 tokens, with a memo.
usdc.transfer(&bob, 25 * scale / 10, Some("invoice 42".into())).await?.result()?;
# Ok(())
# }
```

| Method | Sends | Gas, deposit |
|---|---|---|
| [`transfer(to, amount, memo)`](crate::standards::FungibleTokenClient::transfer) | `ft_transfer` | 30 Tgas, 1 yoctoNEAR |
| [`transfer_call(to, amount, msg)`](crate::standards::FungibleTokenClient::transfer_call) | `ft_transfer_call`, which calls `ft_on_transfer` on the receiver | 100 Tgas, 1 yoctoNEAR |
| [`storage_deposit(account, deposit)`](crate::standards::FungibleTokenClient::storage_deposit) | `storage_deposit` with `registration_only: true` | 30 Tgas, `deposit` |

The 1 yoctoNEAR deposit is what NEP-141 requires, so that a function-call
access key can't move tokens. Each method returns a `Call`, so `.gas(..)`
overrides the default, and `FunctionCall::from(call)` batches it.

`.result()?` on a `transfer_call` doesn't tell you the receiver took the
tokens. If `ft_on_transfer` fails, the token's `ft_resolve_transfer`
callback handles the failure and refunds the sender, so the transaction
still succeeds. Check
[`receipt_failures`](crate::rpc::FinalExecutionOutcome::receipt_failures)
for the receiver's failure, or the amount the call returns (what the
receiver kept, as a JSON string):

```rust,no_run
use near_kit::standards::FungibleToken;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
let usdc = alice.contract::<FungibleToken>(account!("usdc.testnet"));
let outcome = usdc
    .transfer_call(account!("dex.testnet"), 1_000_000_u128, r#"{"action":"deposit"}"#)
    .await?;
if let Some(error) = outcome.receipt_failures().next() {
    return Err(format!("the deposit failed and was refunded: {error}").into());
}
let used: u128 = outcome.json::<String>()?.parse()?;
# let _ = used;
# Ok(())
# }
```

Token views read at `Finality::Optimistic` and have no `.at(..)`; for a
balance at a given block, call `ft_balance_of` with `Near::view` (see the
[finality chapter](crate::guide::finality#reading-at-a-block)). A
contract that returns a non-numeric string for a `U128` field gives
`ContractError::Codec`.

## Non-fungible tokens

[`NonFungibleToken`](crate::standards::NonFungibleToken) works the same
way:

```rust,no_run
use near_kit::standards::NonFungibleToken;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let nft = alice.contract::<NonFungibleToken>(account!("nft.testnet"));

let owned = nft.tokens_for_owner(alice.account_id(), None, Some(50)).await?;
for token in &owned {
    let title = token.metadata.as_ref().and_then(|m| m.title.as_deref());
    println!("{} {}", token.token_id, title.unwrap_or("(untitled)"));
}

if let Some(token) = nft.token("token-1").await? {
    nft.transfer(account!("bob.testnet"), &token.token_id, None).await?.result()?;
}
# Ok(())
# }
```

Views: `metadata` (cached), `token`, `tokens_for_owner(owner, from_index,
limit)`, `total_supply`, `supply_for_owner`. Calls: `transfer`,
`transfer_with_approval` (for an approved account sending someone else's
token), and `transfer_call`, with the same gas and 1 yoctoNEAR deposit as
the FT equivalents. The same caveat applies to `transfer_call`: a failed
`nft_on_transfer` sends the token back and the transaction succeeds, so
check `receipt_failures()` or the `bool` it returns (`true` if the receiver
kept the token).

## See also

- [`contract`](crate::contract), [`Contract`](crate::Contract),
  [`View`](crate::View), [`Call`](crate::Call),
  [`ContractError`](crate::ContractError)
- [`FungibleTokenClient`](crate::standards::FungibleTokenClient),
  [`NonFungibleTokenClient`](crate::standards::NonFungibleTokenClient),
  [`FtMetadata`](crate::standards::FtMetadata),
  [`NftToken`](crate::standards::NftToken)
- [`FunctionCall`](crate::transaction::FunctionCall)
