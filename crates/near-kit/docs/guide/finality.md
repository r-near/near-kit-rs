# Finality and wait levels

Read this when you need to choose how current a read is, how long a send
waits, or why a read right after a write doesn't show it. near-kit's
defaults are consistent: reads look at the latest final block and sends wait
for `Final`, so a default read after a default write sees the write. You
only need this chapter when you change one of them.

## Reading at a block

Every read names a block. [`Near::view`](crate::Near::view),
[`Near::balance`](crate::Near::balance),
[`Near::account_exists`](crate::Near::account_exists) and the
[`View`](crate::View) values typed contract clients return read at
[`BlockReference::default()`](crate::rpc::BlockReference), which is the
latest final block, unless `.at(..)` picks another. `.at(..)` takes
anything that converts into a [`BlockReference`](crate::rpc::BlockReference):

| Argument | Reads |
|---|---|
| [`Finality::Final`](crate::rpc::Finality::Final) (the default) | The latest block that is final: it can't be reverted |
| [`Finality::NearFinal`](crate::rpc::Finality::NearFinal) | The latest block with Doomslug finality: reverted only if validators are slashed |
| [`Finality::Optimistic`](crate::rpc::Finality::Optimistic) | The latest block the node has; newest data, may still be reorganized away |
| a `u64` height | That block |
| a [`CryptoHash`](crate::CryptoHash) | The block with that hash |
| [`SyncCheckpoint::Genesis`](crate::rpc::SyncCheckpoint) or `EarliestAvailable` | The genesis block, or the oldest block the node still stores |

```rust,no_run
use near_kit::rpc::Finality;
use near_kit::*;

# async fn example(near: &Near) -> Result<(), Error> {
let counter = account!("counter.testnet");

let now: u64 = near.view(&counter, "get_count").await?; // final
let newest: u64 = near.view(&counter, "get_count").at(Finality::Optimistic).await?;

// A height: here a recent one, since the default endpoints aren't archival.
let height = near.rpc().block(Finality::Final).await?.header.height;
let then: u64 = near.view(&counter, "get_count").at(height).await?;

// `with_metadata` also returns the block the value was read at.
let read = near.view::<u64>(&counter, "get_count").with_metadata().await?;
println!("{} at block {} ({})", read.result, read.block_height, read.block_hash);

let balance = near.balance(account!("alice.testnet")).at(Finality::Optimistic).await?;
# Ok(())
# }
```

[`RpcClient`](crate::rpc::RpcClient) methods take the block as an argument
instead of `.at(..)`; pass `Finality::Final` for the same default:

```rust,no_run
use near_kit::rpc::Finality;
use near_kit::*;

# async fn example(near: &Near) -> Result<(), Error> {
let account = near.rpc().view_account(&account!("alice.testnet"), Finality::Final).await?;
let block = near.rpc().block(Finality::NearFinal).await?;
println!("{} yocto at height {}", account.amount.as_yoctonear(), block.header.height);
# Ok(())
# }
```

Non-archival RPC nodes, the default FastNEAR endpoints included, drop old
blocks; they keep about five epochs. A read at
an older height or hash fails. near-kit maps the node's `UNKNOWN_BLOCK` and
`GARBAGE_COLLECTED_BLOCK` errors to `RpcError::BlockNotFound`; a node or proxy
that reports it another way gives the catch-all `RpcError::Rpc` (often code
`-32000`, "Server error") or, for a non-JSON HTTP error, `RpcError::Network`.
Use an archival endpoint with [`Near::custom`](crate::Near::custom) for old
blocks.

The FT and NFT clients in [`standards`](crate::standards) are the
exception: their views read at `Finality::Optimistic` and have no `.at(..)`.
For a token balance at a specific block, call the view yourself:

```rust,no_run
use near_kit::*;

# async fn example(near: &Near, block_hash: CryptoHash) -> Result<(), Box<dyn std::error::Error>> {
// NEP-141 returns the balance as a JSON string.
let raw: String = near
    .view(account!("wrap.near"), "ft_balance_of")
    .args(serde_json::json!({ "account_id": "alice.near" }))
    .at(block_hash)
    .await?;
let balance: u128 = raw.parse()?;
# Ok(())
# }
```

## Wait levels

A send returns when the transaction reaches its wait level. The level is a
type, so it also fixes what the send returns:

| Level | Returns when | Response |
|---|---|---|
| [`Submitted`](crate::transaction::Submitted) | The node has accepted the transaction | [`SendTxResponse`](crate::rpc::SendTxResponse), `outcome` is `None` |
| [`Included`](crate::transaction::Included) | It is in a block, which may not be final | `SendTxResponse`, `outcome` may be `None` |
| [`IncludedFinal`](crate::transaction::IncludedFinal) | Its block is final; receipts may still be running | `SendTxResponse`, `outcome` present but partial |
| [`ExecutedOptimistic`](crate::transaction::ExecutedOptimistic) | All its receipts (except refunds) have run, in blocks that may not be final | [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome) |
| [`Executed`](crate::transaction::Executed) | Its block is final and all receipts except refunds have run | `FinalExecutionOutcome` |
| [`Final`](crate::transaction::Final) (the default) | Every receipt, refunds included, has run in a final block | `FinalExecutionOutcome` |

The three executed levels return the full outcome and resolve a rejected
transaction to `Error::InvalidTx`. The three earlier levels return a
[`SendTxResponse`](crate::rpc::SendTxResponse): the transaction hash, the
sender and whatever outcome the node already had. It isn't checked: a
failure in it is not an `Err`.

A [`SigningNear`](crate::SigningNear) carries its level as a type parameter,
`Final` unless you change it. [`wait_until`](crate::SigningNear::wait_until)
returns a client for the same account with another level, and everything
sent through it, typed contract calls included, waits for that level:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::rpc::{FinalExecutionOutcome, SendTxResponse};
use near_kit::transaction::{ExecutedOptimistic, Included};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let bob = account!("bob.testnet");

// Final, the default: a FinalExecutionOutcome.
let outcome: FinalExecutionOutcome =
    alice.transaction(&bob).add_action(Action::transfer(NearToken::from_near(1))).await?;
outcome.result()?;

// Executed but maybe not final yet: faster, same response type.
let alice_fast = alice.wait_until::<ExecutedOptimistic>();
let outcome = alice_fast.transaction(&bob).add_action(Action::transfer(NearToken::from_near(1))).await?;
outcome.result()?;

// In a block: the hash and sender only.
let response: SendTxResponse = alice
    .wait_until::<Included>()
    .transaction(&bob)
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?;
println!("{}", response.transaction_hash);
# Ok(())
# }
```

A pre-signed transaction picks its level on the send:
`near.send_signed(&signed).wait_until::<Included>()`. Without
`wait_until` it also waits for `Final`.

## Reading your own writes

A send at `Final` returns after every receipt has run in a final block, so a
read at the default `Final` sees its effects:

```rust,no_run
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let counter = account!("counter.testnet");
alice
    .transaction(&counter)
    .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(10)))
    .await?.result()?;
let count: u64 = alice.near().view(&counter, "get_count").await?; // includes the increment
# Ok(())
# }
```

If you lower the send's level to save time, lower the read's finality to
match. After an `ExecutedOptimistic` send, a `Final` read can run before
the block holding your receipts is final and return the old value; read at
`Finality::Optimistic` instead:

```rust,no_run
use near_kit::rpc::Finality;
use near_kit::transaction::{ExecutedOptimistic, FunctionCall};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let counter = account!("counter.testnet");
alice
    .wait_until::<ExecutedOptimistic>()
    .transaction(&counter)
    .add_action(FunctionCall::new("increment").gas(Gas::from_tgas(10)))
    .await?.result()?;
let count: u64 = alice.near().view(&counter, "get_count").at(Finality::Optimistic).await?;
# Ok(())
# }
```

Across processes and endpoints there is no such pairing. Two RPC nodes can
be a block or two apart, final blocks included: a node that is behind
answers a `Final` read from its own last final block, which can predate a
write another node already confirmed. No finality makes a read wait for a
height. To see a write through another endpoint, poll the read until it
shows the new state, or read through the endpoint that confirmed it.

Token views read at `Optimistic`, so on the node that ran the send they see
a token transfer sent at any executed level.

## Fire and track

To return as soon as the transaction is in a block and follow it later,
send at an early level and poll [`Near::tx_status`](crate::Near::tx_status)
with the hash:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::transaction::{Final, Included};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let response = alice
    .wait_until::<Included>()
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?;

// Later, possibly in another process: block until it is final.
let outcome = alice
    .near()
    .tx_status(&response.transaction_hash, &response.sender_id)
    .wait_until::<Final>()
    .await?;
outcome.result()?;
println!("{} receipts", outcome.receipts.len());
# Ok(())
# }
```

`tx_status` uses `EXPERIMENTAL_tx_status`, so its outcomes also carry
`receipts` (a send's outcome has only `receipts_outcome`). Awaited without
`wait_until`, it uses `Submitted`: the node answers at once with what it
has, which is how a frontend draws progress without blocking:

```rust,no_run
use near_kit::rpc::RpcError;
use near_kit::*;

# async fn example(near: &Near, hash: CryptoHash) -> Result<(), Error> {
match near.tx_status(&hash, account!("alice.testnet")).await.map_err(Error::into_rpc) {
    Ok(status) => match &status.outcome {
        Some(outcome) => println!("{} receipts so far", outcome.receipts_outcome.len()),
        None => println!("accepted, not executed yet"),
    },
    // The node hasn't seen it yet. Not retried: poll again later.
    Err(Ok(RpcError::TransactionNotFound(_))) => println!("not seen yet"),
    Err(Ok(other)) => return Err(other.into()),
    Err(Err(other)) => return Err(other),
}
# Ok(())
# }
```

## `TxExecutionStatus` is not near-kit's default

[`TxExecutionStatus`](crate::rpc::TxExecutionStatus) is the RPC's own enum,
the `wait_until` value on the wire. Its `Default` is `ExecutedOptimistic`
because that is the node's default when a request leaves `wait_until` out.
near-kit always sends the level explicitly, and its defaults are:

| Where | Default |
|---|---|
| Reads (`View`, `balance`, `account_exists`) | `Finality::Final` |
| Sends from a `SigningNear`, `Near::send_signed` | `Final` |
| `Near::tx_status` | `Submitted` (answer at once) |
| FT and NFT views | `Finality::Optimistic` |

Each wait-level type maps to its status as the constant
[`WaitLevel::STATUS`](crate::transaction::WaitLevel::STATUS).

## See also

- [`BlockReference`](crate::rpc::BlockReference),
  [`Finality`](crate::rpc::Finality), [`View::at`](crate::View::at),
  [`View::with_metadata`](crate::View::with_metadata)
- [`WaitLevel`](crate::transaction::WaitLevel),
  [`SigningNear::wait_until`](crate::SigningNear::wait_until),
  [`SignedTransactionSend`](crate::transaction::SignedTransactionSend),
  [`TransactionStatusQuery`](crate::rpc::TransactionStatusQuery)
- [`SendTxResponse`](crate::rpc::SendTxResponse),
  [`FinalExecutionOutcome`](crate::rpc::FinalExecutionOutcome)
- [Timeouts](crate::guide::errors#timeouts) in the errors chapter
