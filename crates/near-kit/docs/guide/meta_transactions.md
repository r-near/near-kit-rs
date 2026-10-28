# Meta-transactions and relayers

Read this when one account should pay for another account's transaction:
a user signs actions without holding NEAR for gas, and a relayer submits
them and pays. NEAR calls this a meta-transaction (NEP-366). The user signs
a *delegate action* (the receiver, the actions, a nonce and an expiry
height) and hands it over, usually as base64 in an HTTP request. The
relayer wraps it in a transaction of its own.

## The user side: sign a delegate action

[`SigningNear::sign_delegate`](crate::SigningNear::sign_delegate) takes the
receiver, the actions and [`DelegateOptions`](crate::transaction::DelegateOptions),
and returns a [`SignedDelegateAction`](crate::protocol::SignedDelegateAction).
Nothing is sent. [`to_base64`](crate::protocol::SignedDelegateAction::to_base64)
is the encoding to give a relayer:

```rust,no_run
use near_kit::transaction::{DelegateOptions, FunctionCall};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let call = FunctionCall::new("add_message")
    .args(serde_json::json!({ "text": "Hello from a meta-transaction" }))
    .gas(Gas::from_tgas(30));

let signed = alice
    .sign_delegate(account!("guestbook.testnet"), vec![call.into_action()?], DelegateOptions::default())
    .await?;

// POST this string to the relayer.
let payload: String = signed.to_base64();
# Ok(())
# }
```

The actions are ordinary [`Action`](crate::protocol::Action)s; a typed
call becomes one through [`FunctionCall::from`](crate::transaction::FunctionCall),
the same way it is batched into a transaction:

```rust,no_run
use near_kit::standards::FungibleToken;
use near_kit::transaction::FunctionCall;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let wrap = account!("wrap.testnet");
let transfer = alice.contract::<FungibleToken>(&wrap).transfer(account!("bob.testnet"), 1_000_u128, None);
let action = FunctionCall::from(transfer).into_action()?;
let signed = alice.sign_delegate(&wrap, vec![action], Default::default()).await?;
# Ok(())
# }
```

`DelegateOptions` controls the two values a delegate action fixes:

- **Expiry.** The chain rejects the delegate action after
  `max_block_height`. By default it is the current height plus 200 blocks.
  Set `block_height_offset` for another margin, or
  `max_block_height` for an exact height.
- **Nonce.** By default the next nonce for the user's key comes from the
  same process-wide sequence as the user's own transactions (see the
  [nonces chapter](crate::guide::nonces)), so a delegate action signed
  while a transaction from that key is pending doesn't reuse its nonce.
  Set `nonce` to choose it yourself.

```rust,no_run
use near_kit::transaction::DelegateOptions;
use near_kit::protocol::Action;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let options = DelegateOptions {
    block_height_offset: Some(1_000),
    ..Default::default()
};
let signed = alice
    .sign_delegate(account!("bob.testnet"), vec![Action::transfer(NearToken::from_millinear(10))], options)
    .await?;
# Ok(())
# }
```

When both `nonce` and `max_block_height` are set, `sign_delegate` makes no
RPC calls. Otherwise it reads the access key's nonce and the latest block
height. It never retries: a delegate action that fails on chain has to be
signed again.

`sign_delegate` returns `Error::Validation` for an empty action list or for
a delegate action among the actions; delegate actions don't nest. The key
the user signs with must be allowed to run the actions, as if the user sent
them. A function-call access key covers exactly one function call, to its
contract and one of its methods, with no deposit.

## The relayer side: check, then submit

The relayer decodes the payload with
[`SignedDelegateAction::from_base64`](crate::protocol::SignedDelegateAction::from_base64)
and sends a transaction **to the delegate's sender** with one
[`Action::delegate`](crate::protocol::Action::delegate):

```rust,no_run
use near_kit::protocol::{Action, SignedDelegateAction};
use near_kit::*;

# async fn example(relayer: &SigningNear, payload: &str) -> Result<(), Box<dyn std::error::Error>> {
let signed = SignedDelegateAction::from_base64(payload)?;
let outcome = relayer
    .transaction(signed.sender_id())
    .add_action(Action::delegate(signed))
    .await?;
// `outcome.is_success()` says the delegate action was accepted, not that
// the user's actions succeeded; see "Reading the result" below.
println!("relayed in {}", outcome.transaction_hash());
# Ok(())
# }
```

The chain runs the inner actions only when the transaction's receiver is
the delegate's sender. near-kit checks this before signing: any other
receiver is `Error::Validation` and nothing is sent, so a relayer can't pay
for a transaction that is bound to fail that way.

### What to check before paying

Everything inside a delegate action runs at the relayer's expense. The
relayer's transaction pays the inner actions' gas and **their deposits**
too, and when an inner action fails, its deposit is refunded to the user,
not to the relayer. Checks the chain makes (signature, expiry, nonce) fail
as an action error after the relayer has paid for the attempt. So check, in
roughly this order:

1. **The signature.** `signed.signature.verify(..)` over the delegate
   action's hash, with its public key.
2. **The expiry.** `max_block_height` above the current height, and not so
   far above that you'd hold a stale request.
3. **The receiver.** Only contracts you relay for.
4. **The actions.** Only kinds you relay (often function calls only), only
   methods you expect, a gas cap per call and in total, and no deposit
   beyond what you're willing to give away (the 1 yoctoNEAR that FT and
   NFT transfers attach, say).
5. **The key and nonce.** The user's key exists on the sender account with a
   nonce below the delegate's, and you haven't relayed this delegate action
   before. A replay fails on chain with an invalid-nonce action error, at
   your cost.
6. **Your own policy.** Rate limits per sender, allow-lists of users.

The inner actions are [`NonDelegateAction`](crate::protocol::NonDelegateAction)s;
[`inner`](crate::protocol::NonDelegateAction::inner) gives the `Action`:

```rust,no_run
use near_kit::protocol::{Action, SignedDelegateAction};
use near_kit::rpc::Finality;
use near_kit::*;

const MAX_GAS_PER_CALL: Gas = Gas::from_tgas(50);

/// Why a delegate action is refused, or `Ok` to relay it.
async fn check(near: &Near, signed: &SignedDelegateAction) -> Result<(), String> {
    let delegate = &signed.delegate_action;

    let hash = delegate.get_hash();
    if !signed.signature.verify(hash.as_bytes(), &delegate.public_key) {
        return Err("bad signature".into());
    }

    let height = near.rpc().block(Finality::Final).await.map_err(|e| e.to_string())?.header.height;
    if delegate.max_block_height <= height {
        return Err("expired".into());
    }
    if delegate.max_block_height > height + 2_000 {
        return Err("expiry too far ahead".into());
    }

    if delegate.receiver_id.as_str() != "guestbook.testnet" {
        return Err(format!("not relaying to {}", delegate.receiver_id));
    }

    for action in &delegate.actions {
        let Action::FunctionCall(call) = action.inner() else {
            return Err("only function calls are relayed".into());
        };
        if call.method_name != "add_message" {
            return Err(format!("not relaying {}", call.method_name));
        }
        if call.gas > MAX_GAS_PER_CALL {
            return Err("too much gas".into());
        }
        if call.deposit > NearToken::from_yoctonear(1) {
            return Err("deposit too large".into());
        }
    }

    let key = near
        .rpc()
        .view_access_key(&delegate.sender_id, &delegate.public_key, Finality::Final)
        .await
        .map_err(|e| e.to_string())?;
    if key.nonce >= delegate.nonce {
        return Err("nonce already used".into());
    }
    Ok(())
}
```

Then submit as above. For a relayer under load, sign with several keys and
let sends retry nonce conflicts without limit; the
[nonces chapter](crate::guide::nonces#relayers-and-other-high-volume-senders)
has the settings. A relayer that answers its HTTP request early would use
`relayer.wait_until::<Included>()` and track the hash afterwards.

### Reading the result

For a relayed transaction, `outcome.is_failure()` is false even when the
user's call fails. The transaction's final status follows the delegate
receipt, which succeeds once the chain accepts the delegate action and
schedules the inner actions; their own receipt runs afterwards and its
result isn't part of that status. So:

- `outcome.is_failure()` true means the delegate action itself was
  rejected: a bad signature, an expired or reused nonce, a key that may not
  run the actions. `outcome.failure_error()` has the reason, an
  `ActionErrorKind::DelegateAction*` kind.
- `outcome.is_success()` true says only that the inner actions were
  scheduled. Whether they succeeded shows up only in
  `outcome.receipts_outcome`, so a relayer that reports results has to
  inspect the receipts:

```rust,no_run
use near_kit::rpc::{ExecutionStatus, FinalExecutionOutcome};

/// The first failed receipt after the delegate receipt, if any.
fn inner_failure(outcome: &FinalExecutionOutcome) -> Option<String> {
    outcome.receipts_outcome.iter().find_map(|receipt| match &receipt.outcome.status {
        ExecutionStatus::Failure(error) => {
            Some(format!("{} failed: {error:?}", receipt.outcome.executor_id))
        }
        _ => None,
    })
}
```

When the inner call fails, any deposit attached to it goes back to the
user, not to the relayer that paid it.

## Gas-key delegate actions

NEP-611 adds a second delegate action version for gas keys,
[`DelegateActionV2`](crate::protocol::DelegateActionV2), wrapped as
[`VersionedSignedDelegateAction`](crate::protocol::VersionedSignedDelegateAction)
and sent with [`Action::delegate_v2`](crate::protocol::Action::delegate_v2).
`sign_delegate` produces the NEP-366 version only; build and sign a V2
payload yourself with
[`VersionedDelegateActionPayload::get_hash`](crate::protocol::VersionedDelegateActionPayload::get_hash)
and `into_signed`. The same receiver rule applies, and near-kit enforces it
before signing.

## See also

- [`SigningNear::sign_delegate`](crate::SigningNear::sign_delegate),
  [`DelegateOptions`](crate::transaction::DelegateOptions)
- [`SignedDelegateAction`](crate::protocol::SignedDelegateAction),
  [`DelegateAction`](crate::protocol::DelegateAction),
  [`Action::delegate`](crate::protocol::Action::delegate)
- [`Signature::verify`](crate::signer::Signature::verify)
- [`ActionErrorKind`](crate::protocol::ActionErrorKind) for the delegate
  failure kinds
