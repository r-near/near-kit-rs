# Errors and retries

## What a send returns

Awaiting a [`Tx`](crate::transaction::Tx) (or a typed contract call) has
five kinds of result:

| Result | Meaning | On chain |
|---|---|---|
| `Ok(outcome)`, `outcome.is_success()` | Every action ran (receipts off the main chain can still fail; see [below](#failures-that-is_failure-doesnt-see)) | Executed; gas spent, nonce used |
| `Ok(outcome)`, `outcome.is_failure()` | The transaction ran and an action failed: a contract panicked, an account already existed, a deposit was too small | Executed; gas spent, nonce used, the failed receipt's changes rolled back |
| `Err(Error::InvalidTx(e))` | The node rejected the transaction before running it: bad nonce, not enough balance, expired block hash | Nothing ran |
| `Err(Error::Validation(msg))` | near-kit refused to sign it: no actions, or a receiver that can't be right | Nothing was sent |
| any other `Err` | RPC, signing or encoding failure | Usually nothing ran; after a [timeout](#timeouts) it may still run |

The second row is a receipt that failed after the transaction was accepted
and paid for. near-kit reports it as a successful call that returned a
failed outcome, so check the outcome, or match on the result to handle
every case in one place:

```rust,no_run
use near_kit::protocol::{Action, InvalidTxError};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let transfer = alice
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)));

match transfer.await {
    Ok(outcome) if outcome.is_success() => {
        println!("sent in {}", outcome.transaction_hash());
    }
    Ok(outcome) => {
        println!("transfer ran and failed: {:?}", outcome.failure_error());
    }
    Err(Error::InvalidTx(e)) => match *e {
        InvalidTxError::NotEnoughBalance { balance, cost, .. } => {
            println!("need {cost}, have {balance}");
        }
        other => println!("rejected: {other}"),
    },
    Err(Error::Validation(msg)) => println!("not sent: {msg}"),
    Err(e) => return Err(e),
}
# Ok(())
# }
```

### Turning a failed action into an `Err`

When a failed action should stop your code like any other error, read the
return value with [`result`](crate::rpc::FinalExecutionOutcome::result) or
[`json`](crate::rpc::FinalExecutionOutcome::json). Both return
[`OutcomeError::ActionFailed`](crate::rpc::OutcomeError::ActionFailed) for
a failed outcome, and `?` turns that into
[`Error::Outcome`](crate::Error::Outcome):

```rust,no_run
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
// `json` decodes the method's return value; an empty return decodes as `null`.
let new_count: u64 = alice
    .transaction(account!("counter.testnet"))
    .add_action(FunctionCall::new("increment_and_get").gas(Gas::from_tgas(10)))
    .await?
    .json()?;
# Ok(())
# }
```

### Failures that `is_failure` doesn't see

`outcome.status` is the result of the transaction's main chain of
receipts. A receipt that fails off that chain, such as a detached promise,
a callback that a contract handled, or the user's actions in a relayed
meta-transaction (see the [meta-transactions
chapter](crate::guide::meta_transactions#reading-the-result)), leaves
`is_failure()` false and `result()` `Ok`. When that matters,
[`receipt_failures`](crate::rpc::FinalExecutionOutcome::receipt_failures)
lists every failed receipt's action error, in RPC order.

## Which call returns which error

| You call | Error type |
|---|---|
| `Near::balance`, `account_exists`, `tx_status`, `send_signed`; awaiting a `Tx`; `SigningNear::sign`, `sign_delegate`, `from_env` | [`Error`](crate::Error) |
| `near.rpc().view_account(..)` and every other [`RpcClient`](crate::rpc::RpcClient) method | [`RpcError`](crate::rpc::RpcError) |
| `Near::view`, and the views of typed contract and token clients | [`ContractError<RpcError>`](crate::ContractError) |
| `#[call]` methods and token calls through a `SigningNear` | [`ContractError<Error>`](crate::ContractError) |
| `FinalExecutionOutcome::result` / `json` | [`OutcomeError`](crate::rpc::OutcomeError) |
| Client-free code: `FunctionCall::into_action`, key parsing, `RotatingSigner::new`, `SignedTransaction::from_base64`, `SignedDelegateAction::from_base64` | [`CodecError`](crate::transaction::CodecError), [`ParseKeyError`](crate::signer::ParseKeyError), [`SignerError`](crate::signer::SignerError), [`DecodeTransactionError`](crate::protocol::DecodeTransactionError), [`DecodeDelegateError`](crate::protocol::DecodeDelegateError) |

Each of these converts into [`Error`](crate::Error) with `?`, so a function
returning `Result<_, near_kit::Error>` can mix them freely. A
`DecodeTransactionError` becomes [`Error::Decode`](crate::Error::Decode) and a
`DecodeDelegateError` [`Error::DecodeDelegate`](crate::Error::DecodeDelegate).

[`ContractError<E>`](crate::ContractError) has two cases: `Codec` (the
arguments didn't encode or the result didn't decode as the type you asked
for) and `Backend(E)` (the backend's own error). It exists because a typed
client runs on any [`Viewer`](crate::Viewer) or
[`TxSender`](crate::TxSender), whose error types near-kit doesn't know.

```rust,no_run
use near_kit::rpc::RpcError;
use near_kit::*;

# async fn example(near: &Near) -> Result<(), Error> {
match near.view::<u64>(account!("counter.testnet"), "get_count").await {
    Ok(count) => println!("{count}"),
    Err(ContractError::Codec(e)) => println!("not a u64: {e}"),
    Err(ContractError::Backend(RpcError::MethodNotFound { method_name, .. })) => {
        println!("no method {method_name}");
    }
    Err(e) => return Err(e.into()),
}
# Ok(())
# }
```

## `InvalidTx`, `Validation` and RPC errors

These three are all `Err`, and they come from different places.

[`Error::InvalidTx`](crate::Error::InvalidTx) holds a nearcore
[`InvalidTxError`](crate::protocol::InvalidTxError): the node, or the
chunk that included the transaction, refused it. The transaction didn't run.
A few of these, like `InvalidNonce` and `Expired`, near-kit retries for you
before you see them (see [Retries](#retries)); the rest, like
`NotEnoughBalance` or `InvalidAccessKeyError`, won't change on a second try.

[`Error::Validation`](crate::Error::Validation) is near-kit's own check
before signing, so nothing reached the network; the
[transactions chapter](crate::guide::transactions#checks-before-signing) lists the checks.

[`Error::Rpc`](crate::Error::Rpc) boxes an [`RpcError`](crate::rpc::RpcError):
the network failed, the node returned an error, or the response didn't
parse. Reach it with [`as_rpc`](crate::Error::as_rpc) or
[`into_rpc`](crate::Error::into_rpc) rather than matching through the box.
Common read errors are variants you can match:

```rust,no_run
use near_kit::*;

/// Whether `account_id` has a contract deployed.
async fn has_contract(near: &Near, account_id: &AccountId) -> Result<bool, Error> {
    match near.rpc().view_code(account_id, rpc::Finality::Final).await {
        Ok(_) => Ok(true),
        Err(rpc::RpcError::ContractNotDeployed { .. } | rpc::RpcError::AccountNotFound { .. }) => {
            Ok(false)
        }
        Err(e) => Err(e.into()),
    }
}
```

Converting an `RpcError` into an `Error` promotes a structured
`RpcError::InvalidTx` to `Error::InvalidTx`, so after `?` you match one
variant, not two.

A view that panics is different from a call that panics: a view has no
transaction, so its panic comes back as `RpcError::ContractPanic` (or
`MethodNotFound`, `ContractNotDeployed`, `ContractExecution`), while a call's
panic is inside an `Ok(outcome)`.

## Retries

near-kit retries at two layers, and they don't overlap.

**The RPC layer** re-sends the same request. It runs inside every
[`RpcClient`](crate::rpc::RpcClient) method, which means every read and
every `send_tx`, and it is controlled by
[`RetryConfig`](crate::rpc::RetryConfig). The default is three retries after
the first attempt, waiting 500 ms, 1 s and 2 s. A request is retried when
[`RpcError::is_retryable`](crate::rpc::RpcError::is_retryable) is true:

- transport failures: connection refused, reset or dropped mid-response, and
  client timeouts;
- HTTP 408, 429 and 5xx responses that don't carry a JSON-RPC error, and
  408 and 429 responses whose JSON-RPC error has no variant of its own;
- `ShardUnavailable`, `NodeNotSynced`, `InternalError` and `Timeout`;
- `InvalidTx` with `ShardCongested` or `ShardStuck`;
- `RpcError::Rpc` with code `-32603`, and `RpcError::Rpc` with code
  `-32000` when its `cause` is absent or names a condition that clears with
  time: `NODE_IS_SYNCING`, `NO_NEW_BLOCKS`, `BUSY`, `NOT_CONFIRMED`,
  `CHUNK_NOT_CERTIFIED` (and a few that near-kit normally maps to their own
  variants). A `-32000` error with any other cause is returned at once,
  since it depends on the request or the node and won't change.

Everything else is returned after one attempt, including
`TransactionNotFound`, `ShardNotTracked`, account and contract errors, and
`InvalidTx(InvalidNonce)`: the signed bytes are fixed at this layer and the
same nonce can't succeed twice. Re-sending a signed transaction is safe,
because the node recognises its hash and reports its status rather than
running it twice. `sandbox_fast_forward` is the one method never retried.

**The transaction layer** re-signs. When the node rejects a send with
`InvalidNonce`, `NonceTooLarge` or `Expired`, a `Tx` signs again with a
fresh nonce or block hash, up to `max_nonce_retries` times; see the
[nonces chapter](crate::guide::nonces#nonce-retries).

### Configuring RPC retries

Pass a [`RetryConfig`](crate::rpc::RetryConfig) to the builder. Use
[`RetryConfig::none`](crate::rpc::RetryConfig::none) when your code owns the
retry policy, so near-kit doesn't add delay before you see the error:

```rust
use near_kit::Near;
use near_kit::rpc::RetryConfig;

// A CLI that asks the user before retrying.
let near = Near::testnet().retry_config(RetryConfig::none()).build();

// A batch job that rides out longer outages: 500 ms doubling to 10 s, ten times.
let patient = Near::testnet()
    .retry_config(RetryConfig {
        max_retries: 10,
        initial_delay_ms: 500,
        max_delay_ms: 10_000,
    })
    .build();
```

### Retrying in your own code

[`Error`](crate::Error) has no `is_retryable`; ask the RPC error inside it.
With the default `RetryConfig` a retryable error has already been retried
before it reaches you, so a loop of your own mostly makes sense on a client
built with `RetryConfig::none()`, where you choose the backoff:

```rust,no_run
use near_kit::*;

async fn balance_with_retries(near: &Near, id: &AccountId) -> Result<NearToken, Error> {
    let mut attempt = 0;
    loop {
        match near.balance(id).await {
            Ok(balance) => return Ok(balance.available),
            Err(e) if attempt < 5 && e.as_rpc().is_some_and(|e| e.is_retryable()) => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_secs(attempt)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Timeouts

[`RpcError::Timeout`](crate::rpc::RpcError::Timeout) from a send means the
node gave up waiting for your [wait level](crate::guide::finality), not that
the transaction failed. It may already be in a block. Don't sign a new
transaction for the same intent; poll the old one by hash. The error carries
`transaction_hash`, and nodes running nearcore 2.14 or later say how far the
transaction got in `cause`. `Timeout` is retryable, so with the default
`RetryConfig` the same signed transaction has already been re-sent three
times before you see it:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::rpc::{RpcError, TimeoutCause};
use near_kit::transaction::Final;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let result = alice
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await;

let outcome = match result.map_err(Error::into_rpc) {
    Ok(outcome) => outcome,
    Err(Ok(RpcError::Timeout { transaction_hash: Some(hash), cause, .. })) => {
        if let Some(TimeoutCause::Pending { final_execution_status, .. }) = &cause {
            println!("accepted, reached {final_execution_status:?}; waiting");
        }
        alice
            .near()
            .tx_status(&hash, alice.account_id())
            .wait_until::<Final>()
            .await?
    }
    Err(Ok(other)) => return Err(other.into()),
    Err(Err(other)) => return Err(other),
};
outcome.result()?;
# Ok(())
# }
```

`TimeoutCause::NotObserved` means the node never saw the transaction on
chain; re-sending the same signed transaction (with `Near::send_signed`) is
safe then. `TimeoutCause::Pending` means it was accepted: wait for it.

## Your own error type

If your error enum wraps `near_kit::Error` with a `From` impl, `?` reaches
it from anything that returns `Error`. It doesn't reach it from a view or a
typed call: `ContractError<RpcError>` to `Error` to your enum is two
conversions, and `?` makes one. Convert first, or add the second `From`
once:

```rust,no_run
use near_kit::rpc::RpcError;
use near_kit::*;

#[derive(Debug)]
enum AppError {
    Near(near_kit::Error),
    CounterTooHigh(u64),
}

impl From<near_kit::Error> for AppError {
    fn from(e: near_kit::Error) -> Self {
        AppError::Near(e)
    }
}

// Lets `?` work on views without a `map_err`.
impl From<ContractError<RpcError>> for AppError {
    fn from(e: ContractError<RpcError>) -> Self {
        AppError::Near(e.into())
    }
}

async fn check(near: &Near) -> Result<u64, AppError> {
    let balance = near.balance(account!("counter.testnet")).await?; // Error
    let count: u64 = near.view(account!("counter.testnet"), "get_count").await?; // ContractError
    if count > 100 {
        return Err(AppError::CounterTooHigh(count));
    }
    println!("{} available", balance.available);
    Ok(count)
}
```

Without the second impl, write `.await.map_err(near_kit::Error::from)?`.

Every near-kit error implements `std::error::Error`, including
`ContractError<E>` whenever `E` is `Debug + Display`, so `?` into
`Box<dyn std::error::Error>` or `anyhow::Error` works without either step.
