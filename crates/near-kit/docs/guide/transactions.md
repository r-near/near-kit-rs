# Transactions

## Building and sending

[`SigningNear::transaction`](crate::SigningNear::transaction) starts a
[`Tx`](crate::Tx) to a receiver;
[`add_action`](crate::Tx::add_action) appends an action;
awaiting it fetches a nonce and block hash, signs, sends and waits for the
client's [wait level](crate::guide::finality) (`Final` by default):

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::SecretKey;
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
// Create a sub-account, fund it, give it a key, deploy and initialize a
// contract: one transaction, so all of it happens or none of it does.
let key = SecretKey::generate_ed25519();
let wasm = std::fs::read("contract.wasm")?;
alice
    .transaction(account!("app.alice.testnet"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(5)))
    .add_action(Action::add_full_access_key(key.public_key()))
    .add_action(Action::deploy_contract(wasm))
    .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner_id": alice.account_id() })))
    .await?
    .result()?;
# Ok(())
# }
```

All actions in a transaction run in one receipt on the receiver, in order.
If one fails, the receipt's state changes are rolled back, including the
earlier actions, and the outcome reports the failure (see the
[errors chapter](crate::guide::errors)). Calls a contract makes to other
contracts run later, in receipts of their own, and are not rolled back with
it.

`add_action` takes an `Action` or anything that converts into one, like a
[`FunctionCall`](crate::transaction::FunctionCall). It never fails: if a
call's arguments fail to encode, the error is kept and returned from
`.await`, and nothing is sent.

To build a transaction in a loop, reassign it:

```rust,no_run
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear, messages: Vec<String>) -> Result<(), Error> {
let mut tx = alice.transaction(account!("guestbook.testnet"));
for text in &messages {
    tx = tx.add_action(
        FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": text }))
            .gas(Gas::from_tgas(10)),
    );
}
tx.await?.result()?;
# Ok(())
# }
```

The protocol caps the total gas one transaction can prepay for its
function calls, so a long batch may need splitting into several
transactions.

### Through any sender

`Tx` works with any [`TxSender`](crate::TxSender), not only `SigningNear`:
a wallet, a gateway or a test double builds transactions with the same
[`transaction`](crate::TxSender::transaction) and `add_action`, and
`.await` returns that sender's own output and error. A function generic
over its sender takes it by reference:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_kit::*;

async fn bump<S: TxSender>(sender: &S) -> Result<S::Output, S::SendError> {
    sender
        .transaction(account!("counter.testnet"))
        .add_action(FunctionCall::new("bump").args(serde_json::json!({ "by": 2 })))
        .add_action(Action::transfer(NearToken::from_millinear(10)))
        .await
}
```

`TxSender::transaction` borrows the sender, so its `Tx` awaits but can't
outlive it. `SigningNear::transaction` clones the client into the `Tx`
instead, so that one can be spawned or returned; through an
`Arc<SigningNear>`, write `(*alice).transaction(..)` to get it. For any
other owned sender, start from [`Tx::new`](crate::Tx::new). A sender that
implements [`WithWaitLevel`](crate::WithWaitLevel), as `SigningNear` does,
also supports [`Tx::wait_until`](crate::Tx::wait_until) for one
transaction.

### A sender picked at run time

`TxSender` has `async` methods, so `dyn TxSender` isn't a type. To hold a
sender chosen at run time, say your own wallet sender or a `SigningNear`,
put a small object-safe trait in front of it and implement `TxSender` for
that trait object. Every sender whose error converts into a
`Box<dyn std::error::Error + Send + Sync>` (any `Error + Send + Sync`
type, or that box itself) erases to the same two types: its output
converts into a [`SendTxResponse`](crate::rpc::SendTxResponse), and its
error is boxed.

```rust,no_run
use std::future::Future;
use std::sync::Arc;

use near_kit::protocol::Action;
use near_kit::rpc::{BoxFuture, SendTxResponse};
use near_kit::transaction::{FunctionCall, Included};
use near_kit::*;

/// The error of a type-erased sender.
pub type DynSendError = Box<dyn std::error::Error + Send + Sync>;

/// An object-safe `TxSender`: every `TxSender` whose error converts into
/// a `DynSendError` implements it.
pub trait DynTxSender: MaybeSend + MaybeSync {
    fn dyn_account_id(&self) -> &AccountId;

    fn dyn_chain_id(&self) -> &str;

    fn dyn_send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynSendError>>;
}

impl<S> DynTxSender for S
where
    S: TxSender,
    S::SendError: Into<DynSendError>,
{
    fn dyn_account_id(&self) -> &AccountId {
        self.account_id()
    }

    fn dyn_chain_id(&self) -> &str {
        self.chain_id()
    }

    fn dyn_send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynSendError>> {
        Box::pin(async move {
            match self.send(receiver_id, actions).await {
                Ok(output) => Ok(output.into()),
                Err(error) => Err(error.into()),
            }
        })
    }
}

impl TxSender for dyn DynTxSender + '_ {
    type Output = SendTxResponse;
    type SendError = DynSendError;

    fn account_id(&self) -> &AccountId {
        self.dyn_account_id()
    }

    fn chain_id(&self) -> &str {
        self.dyn_chain_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<SendTxResponse, DynSendError>> + MaybeSend {
        self.dyn_send(receiver_id, actions)
    }
}

/// A shared, type-erased sender.
pub type ArcTxSender = Arc<dyn DynTxSender>;

# struct MyWallet(AccountId);
# impl TxSender for MyWallet {
#     type Output = SendTxResponse;
#     type SendError = DynSendError;
#     fn account_id(&self) -> &AccountId { &self.0 }
#     fn chain_id(&self) -> &str { "testnet" }
#     async fn send(&self, _: AccountId, _: Vec<Action>) -> Result<SendTxResponse, DynSendError> {
#         Ok(SendTxResponse::new(CryptoHash::ZERO, self.0.clone(), None))
#     }
# }
// `MyWallet` is your own `TxSender`, with its own output and error.
fn pick(alice: SigningNear, wallet: Option<MyWallet>) -> ArcTxSender {
    match wallet {
        Some(wallet) => Arc::new(wallet),
        // Pick the wait level before erasing: `wait_until` doesn't survive it.
        None => Arc::new(alice.wait_until::<Included>()),
    }
}

# async fn example(alice: SigningNear, wallet: Option<MyWallet>) -> Result<(), DynSendError> {
let sender = pick(alice, wallet);
let sent = sender
    .transaction(account!("counter.testnet"))
    .add_action(FunctionCall::new("bump"))
    .await?;
println!("{} signed {}", sent.sender_id, sent.transaction_hash);
# Ok(())
# }
```

`ArcTxSender` is then a sender like any other: generic code takes it,
`Tx::new(Arc::clone(&sender), id)` builds a transaction that can be
spawned, and since it is `Clone`, a `#[contract]` client accepts it.
Erasing a sender changes three things:

- **The output** is a `SendTxResponse`: the hash and signer, plus the
  execution outcome when the sender waited for one.
- **The error** downcasts back to the sender's own:
  `error.downcast_ref::<near_kit::Error>()` for a `SigningNear`. An
  argument that failed to encode never reaches the sender, so it downcasts
  to [`CodecError`](crate::transaction::CodecError).
- **The wait level** can't change after the sender is erased.
  `ArcTxSender` doesn't implement [`WithWaitLevel`](crate::WithWaitLevel),
  so pick the level first, as `pick` does.

Share it through `Arc`, `Box` or `&`, which forward `TxSender`; `Rc`
doesn't. A [`Viewer`](crate::Viewer) erases the same way. Write the
`view_function` of its trait object as an `async fn` that awaits the boxed
future: returning that future directly fails with E0700, since its single
lifetime ties together four borrows that the trait keeps separate.

## The actions

| Constructor | Does | Send to |
|---|---|---|
| [`create_account()`](crate::protocol::Action::create_account) | Creates the receiver | A new sub-account of the signer |
| [`transfer(amount)`](crate::protocol::Action::transfer) | Sends NEAR | Anyone. To a 64-hex implicit account, it also creates it |
| [`function_call(method, args, gas, deposit)`](crate::protocol::Action::function_call), or a [`FunctionCall`](crate::transaction::FunctionCall) | Calls a contract method | The contract |
| [`deploy_contract(wasm)`](crate::protocol::Action::deploy_contract) | Deploys code | An account the signer controls |
| [`add_full_access_key(pk)`](crate::protocol::Action::add_full_access_key) | Adds a key that can do anything | An account the signer controls |
| [`add_function_call_key(pk, receiver, methods, allowance)`](crate::protocol::Action::add_function_call_key) | Adds a key that can only call `receiver`, without deposits | An account the signer controls |
| [`delete_key(pk)`](crate::protocol::Action::delete_key) | Removes a key | An account the signer controls |
| [`delete_account(beneficiary)`](crate::protocol::Action::delete_account) | Deletes the receiver, sending its balance to `beneficiary` | The account to delete |
| [`stake(amount, validator_key)`](crate::protocol::Action::stake) | Stakes as a validator | The signer's own account |
| [`publish(wasm, mode)`](crate::protocol::Action::publish), [`deploy_from(id)`](crate::protocol::Action::deploy_from) | Global contracts (below) | See below |
| [`state_init(..)`](crate::protocol::Action::state_init), [`universal_state_init(..)`](crate::protocol::Action::universal_state_init) | Deterministic and universal accounts (below) | The derived account |
| [`delegate(signed)`](crate::protocol::Action::delegate), [`delegate_v2(..)`](crate::protocol::Action::delegate_v2) | Relays a meta-transaction | The delegate's sender ([meta-transactions chapter](crate::guide::meta_transactions)) |
| [`transfer_to_gas_key(pk, amount)`](crate::protocol::Action::transfer_to_gas_key), [`withdraw_from_gas_key(pk, amount)`](crate::protocol::Action::withdraw_from_gas_key) | Funds or drains a gas key | The key's account |

A gas key (NEP-611) is an access key with a NEAR balance of its own that
pays for the gas of the transactions it signs, and several nonces so it can
sign in parallel. The last row funds that balance and takes it back.

An account can create only its own sub-accounts: `alice.testnet` can
create `app.alice.testnet`, not `app.testnet`. Top-level names are created
by calling the `testnet` or `near` account's `create_account` method.

### Function calls

[`FunctionCall`](crate::transaction::FunctionCall) builds a call without a
client. Arguments are JSON with `args`, Borsh with `args_borsh`, or raw
bytes with `args_raw`. Gas defaults to 30 Tgas and the deposit to zero:

```rust,no_run
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
alice
    .transaction(account!("market.testnet"))
    .add_action(
        FunctionCall::new("buy")
            .args(serde_json::json!({ "item": 7 }))
            .gas(Gas::from_tgas(100))
            .deposit(NearToken::from_near(2)),
    )
    .await?.result()?;
# Ok(())
# }
```

Typed calls batch the same way. A call from a typed contract client or a
token client is a [`Call`](crate::Call); `FunctionCall::from(call)` keeps
its method, arguments, gas and deposit, and drops the client.

A `#[near_kit::contract]` trait also generates a `FunctionCall`
constructor for each `#[call]` method, such as `Counter::add(value)`; see
the [contracts chapter](crate::guide::contracts_and_tokens).

### Access keys

A function-call access key lets an app act for a user on one contract
without full control of the account. It can call only `receiver` (and only
`methods`, unless the list is empty), never attach a deposit, and spend at
most `allowance` on gas (`None` for no limit):

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::SecretKey;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let app_key = SecretKey::generate_ed25519();
alice
    .transaction(alice.account_id())
    .add_action(Action::add_function_call_key(
        app_key.public_key(),
        account!("guestbook.testnet"),
        vec!["add_message".to_string()],
        Some(NearToken::from_millinear(250)),
    ))
    .await?.result()?;

// Later: revoke it.
alice
    .transaction(alice.account_id())
    .add_action(Action::delete_key(app_key.public_key()))
    .await?.result()?;
# Ok(())
# }
```

For keys with other permissions, such as gas keys, build the
[`AddKeyAction`](crate::protocol::AddKeyAction) with an
[`AccessKey`](crate::protocol::AccessKey) and
[`AccessKeyPermission`](crate::protocol::AccessKeyPermission) yourself and
wrap it in `Action::AddKey`.

## Global contracts

A global contract is code published once and used by many accounts, each
paying only for a reference instead of storing the code. Publish it with
[`Action::publish`](crate::protocol::Action::publish), sent to your own
account:

- [`PublishMode::Updatable`](crate::protocol::PublishMode) identifies it by
  your account. Accounts that use it follow your later publishes.
- [`PublishMode::Immutable`](crate::protocol::PublishMode) identifies it by
  the code's hash. It can never change.

An account then uses it with
[`Action::deploy_from`](crate::protocol::Action::deploy_from), passing the
publisher's account or the code hash:

```rust,no_run
use near_kit::protocol::{Action, PublishMode};
use near_kit::*;

# async fn example(publisher: &SigningNear, alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
let wasm = std::fs::read("contract.wasm")?;
let code_hash = CryptoHash::hash(&wasm);

publisher
    .transaction(publisher.account_id())
    .add_action(Action::publish(wasm.clone(), PublishMode::Updatable))
    .add_action(Action::publish(wasm, PublishMode::Immutable))
    .await?.result()?;

// Follows the publisher's updates:
alice
    .transaction(alice.account_id())
    .add_action(Action::deploy_from(publisher.account_id().clone()))
    .await?.result()?;

// Pinned to this exact code:
alice
    .transaction(account!("pinned.alice.testnet"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(1)))
    .add_action(Action::deploy_from(code_hash))
    .await?.result()?;

// Read published code back.
let code = alice.near().rpc().view_global_contract_code(code_hash, rpc::Finality::Final).await?;
# Ok(())
# }
```

## Deterministic accounts (NEP-616)

A deterministic account's ID is derived from its initial state: a global
contract reference plus storage entries. Anyone can create it, and anyone
who knows the state knows the ID in advance. Build the
[`StateInit`](crate::protocol::StateInit), derive the ID from it with
[`derive_deterministic_account_id`](crate::protocol::derive_deterministic_account_id),
and send [`Action::state_init`](crate::protocol::Action::state_init) **to that
ID**:

```rust,no_run
use near_kit::protocol::{Action, StateInit, StateInitV1, derive_deterministic_account_id};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let state_init: StateInit = StateInitV1::code(account!("wallets.publisher.testnet"))
    .with_data_entry(b"owner".to_vec(), alice.account_id().as_str().as_bytes().to_vec())
    .into();
let account_id = derive_deterministic_account_id(&state_init); // "0s..."

alice
    .transaction(&account_id)
    .add_action(Action::state_init(state_init, NearToken::from_millinear(100)))
    .await?.result()?;
# Ok(())
# }
```

The ID depends only on the state, not on the deposit, so you can also
compute it just to look the account up.
`derive_deterministic_account_id` hashes in pure Rust. near-global-contracts'
`StateInit::derive_account_id` gives the same ID, but it calls NEAR host
functions when any crate in the workspace enables `near-sdk/unit-testing`,
and a test binary that doesn't link near-sdk then fails to link.

## Universal accounts

A `0u` universal account (nearcore 2.14) is likewise named by the hash of
its initial state, a [`UniversalStateInit`](crate::protocol::UniversalStateInit):
full-access keys, and optionally a global contract and storage. Another
account can create it by sending
[`Action::universal_state_init`](crate::protocol::Action::universal_state_init)
to the derived ID:

```rust,no_run
use near_kit::protocol::{Action, UniversalStateInit, UniversalStateInitV1};
use near_kit::signer::SecretKey;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let key = SecretKey::generate_ed25519();
let state_init: UniversalStateInit = UniversalStateInitV1::new().with_access_key(key.public_key()).into();
let account_id = state_init.derive_account_id(); // "0u..."

alice
    .transaction(&account_id)
    .add_action(Action::universal_state_init(state_init, NearToken::from_millinear(100)))
    .await?.result()?;
# Ok(())
# }
```

An account can also set itself up. Fund the `0u` ID with a transfer first,
which creates it uninitialized, then send the state init from the account
itself, signed with a key the state init installs. That key isn't on chain
yet, so near-kit takes the nonce from the uninitialized account's
`bootstrap_nonce` instead of an access key:

```rust,no_run
use near_kit::protocol::{Action, UniversalStateInit, UniversalStateInitV1};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let key = SecretKey::generate_ed25519();
let state_init: UniversalStateInit = UniversalStateInitV1::new().with_access_key(key.public_key()).into();
let account_id = state_init.derive_account_id();

alice
    .transaction(&account_id)
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?.result()?;

let universal = alice.with_signer(InMemorySigner::new(&account_id, key));
universal
    .transaction(&account_id)
    .add_action(Action::universal_state_init(state_init, NearToken::ZERO))
    .await?.result()?;
# Ok(())
# }
```

near-sdk passes state inits as
[`RawStateInit`](crate::protocol::RawStateInit) bytes;
`Action::universal_state_init` accepts those as well, and
[`UniversalStateInit::from_raw`](crate::protocol::UniversalStateInit::from_raw)
decodes them.

## Checks before signing

Awaiting a `Tx`, `SigningNear::sign` and `sign_delegate` check the actions
first. These fail with [`Error::Validation`](crate::Error::Validation)
before anything is signed or sent:

- no actions;
- a state init (`state_init` or `universal_state_init`) sent to any
  receiver other than the account it derives;
- a delegate action sent to any receiver other than its sender;
- a delegate action inside `sign_delegate`'s actions.

Each of these would otherwise reach the chain and fail there, after the
signer had paid for it. Everything else is checked by the node: a rejected
transaction is `Error::InvalidTx`, and an action that fails while running
is a failed `Ok(outcome)`.
