# Nonces and concurrency

For one transaction at a time there is nothing to do: near-kit picks the
nonce, and re-signs if the node rejects it.

## How near-kit picks a nonce

Every access key has a nonce. A transaction signed with that key must carry
a larger nonce than the key's current one, and the chain then stores the
transaction's nonce as the key's. (It must also stay below the block height
times 1,000,000, which only matters after a nonce jumps far ahead.)

Before signing, a send views the access key at the final block and asks a
process-wide nonce cache for the next nonce: one more than the larger of the
cached value and the chain's. The cache is updated under a lock, so
concurrent sends from one process get distinct nonces even though they all
read the same, not yet updated, value from the chain.

The cache is keyed by **the chain's genesis hash, account and public key**.
Each client asks its node for the genesis hash (`status`) before the first
nonce it draws, and keeps it. That means:

- Clones of a `SigningNear`, and separate `SigningNear`s built for the same
  key, share one sequence.
- Clients pointed at different RPC URLs for the same network share it too,
  whatever chain ID they were given: every node of a network reports the
  same genesis hash.
- Two networks never share it, even with one chain ID (two local sandboxes,
  say).
- A backend that can't report its genesis hash (it answers `status` with
  "method not found", as a canned test transport might) shares nonces by
  chain ID instead. So with [`Near::custom`](crate::Near::custom), still
  pass the network's real chain ID, not a label per endpoint.
- Any other `status` failure fails that send or signature before a nonce
  is drawn, and the next one asks again.
- Two processes never share it. Each starts from the chain's view.

The nonce is taken from the cache when the transaction is signed, so
`SigningNear::sign` and `SigningNear::sign_delegate` advance the sequence
too. A nonce that is never used leaves a gap, which the chain doesn't mind.

## Nonce retries

When the node rejects a send for one of these reasons, the send signs again
and sends again:

| Rejection | Cause | What the retry does |
|---|---|---|
| `InvalidNonce` | Another transaction with this key landed first (another task, process, or a stale view) | Takes the nonce the node reported as a floor, fetches a fresh view, signs with the next nonce |
| `NonceTooLarge` | The cached nonce is past what this chain accepts (cached by chain ID, for a backend that can't report its genesis hash, so possibly from another chain with that ID) | Drops the cached nonce and starts again from the chain's view |
| `Expired` | The block hash the transaction referenced is too old | Fetches a fresh block hash and nonce |

The number of retries is `max_nonce_retries`, 3 by default (so up to four
attempts); `0` sends once. Each retry uses the same key as the first
attempt, even with a [`RotatingSigner`](crate::signer::RotatingSigner). Any
other rejection, and the last allowed attempt's rejection, comes back as
`Error::InvalidTx`.

Set it for every signing client made from a `Near`, or for one sender:

```rust,no_run
use near_kit::signer::InMemorySigner;
use near_kit::*;

# fn example(key: signer::SecretKey) {
// Every SigningNear from this Near retries up to 10 times.
let near = Near::testnet().max_nonce_retries(10).build();
let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), key));

// The same account with its own policy; `alice` keeps 10.
let alice_retrying = alice.clone().max_nonce_retries(50);
# }
```

Only sends from a `SigningNear` (a [`Tx`](crate::transaction::Tx), or a
typed contract call through one) retry like this. `sign`, `sign_delegate`
and [`Near::send_signed`](crate::Near::send_signed) never re-sign: the
signature fixes the nonce. Separately, the RPC layer re-sends the *same*
signed bytes on network errors; the [errors chapter](crate::guide::errors#retries)
describes both layers.

## Concurrent sends from one key

Several sends from one key at once work: each gets its own nonce from the
cache.

```rust,no_run
use std::future::IntoFuture;

use futures::future::try_join_all;
use near_kit::protocol::Action;
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Error> {
let recipients = [account!("bob.testnet"), account!("carol.testnet"), account!("dave.testnet")];
let sends = recipients.iter().map(|to| {
    alice
        .transaction(to)
        .add_action(Action::transfer(NearToken::from_millinear(10)))
        .into_future()
});
for outcome in try_join_all(sends).await? {
    outcome.result()?;
}
# Ok(())
# }
```

A `Tx` is [`IntoFuture`](std::future::IntoFuture), not a `Future`, so
`.await` works on it directly; call `.into_future()` to hand it to a
combinator such as `try_join_all`, or to `tokio::spawn`.

They don't necessarily land in the order you started them. If the
transaction with nonce 12 reaches a validator before the one with nonce 11,
nonce 11 is now too low: the node rejects it with `InvalidNonce`, and
near-kit re-signs it with nonce 13. With many sends in flight on one key,
some can run out of retries. Three ways to handle that, from simplest:

- **Send in order.** If transaction B must follow A, await A before sending
  B. At the default `Final` level the next send starts only after the
  previous one has fully executed.
- **More keys.** Each access key has its own nonce, so N keys give N
  independent sequences. See the next section.
- **More retries.** Raise `max_nonce_retries` and bound the total time with
  a timeout of your own (see [Relayers](#relayers-and-other-high-volume-senders)).

## Several keys for one account

A [`RotatingSigner`](crate::signer::RotatingSigner) holds several keys of
one account and hands out the next one, round-robin, for each send. Add the
keys to the account first:

```rust,no_run
use std::future::IntoFuture;

use futures::future::try_join_all;
use near_kit::protocol::Action;
use near_kit::signer::{RotatingSigner, SecretKey};
use near_kit::*;

# async fn example(bot: &SigningNear) -> Result<(), Error> {
// Three more full-access keys for the bot's account, in one transaction.
let keys: Vec<SecretKey> = (0..3).map(|_| SecretKey::generate_ed25519()).collect();
let mut add_keys = bot.transaction(bot.account_id());
for key in &keys {
    add_keys = add_keys.add_action(Action::add_full_access_key(key.public_key()));
}
add_keys.await?.result()?;

// A client for the same account that rotates through them.
let bot = bot.near().with_signer(RotatingSigner::new(bot.account_id(), keys)?);
let sends = (0..30).map(|_| {
    bot.transaction(account!("bob.testnet"))
        .add_action(Action::transfer(NearToken::from_millinear(1)))
        .into_future()
});
for outcome in try_join_all(sends).await? {
    outcome.result()?;
}
# Ok(())
# }
```

`bot.signer().public_key()` shows the key the next send will claim,
without claiming it. For keys stored elsewhere,
[`RotatingSigner::from_signers`](crate::signer::RotatingSigner::from_signers)
combines `InMemorySigner`s loaded with `signer::from_credentials_file` or
`signer::from_keyring`.

Rotation spreads sends over keys but doesn't order them. When each key's
transactions must land in order, split the signer into one
`InMemorySigner` per key with
[`into_per_key_signers`](crate::signer::RotatingSigner::into_per_key_signers)
and give each key its own queue, sending one transaction at a time per key:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::RotatingSigner;
use near_kit::transaction::Included;
use near_kit::*;

# async fn example(near: &Near, bot_keys: RotatingSigner) -> Result<(), Box<dyn std::error::Error>> {
let mut workers = Vec::new();
for signer in bot_keys.into_per_key_signers() {
    // The bot's account, one key per task. Waiting for `Included` is
    // enough to keep one key's nonces in order.
    let bot = near.with_signer(signer).wait_until::<Included>();
    workers.push(tokio::spawn(async move {
        for _ in 0..10 {
            bot.transaction(account!("bob.testnet"))
                .add_action(Action::transfer(NearToken::from_millinear(1)))
                .await?;
        }
        Ok::<_, Error>(())
    }));
}
for worker in workers {
    worker.await??;
}
# Ok(())
# }
```

## Several processes

The cache lives in one process. Two processes signing with the same key
each start from the chain's nonce and collide; every collision costs a
rejected attempt and a retry. Give each process, or each replica of a
service, its own access key, and they never collide.

## Relayers and other high-volume senders

A relayer sends many transactions from its own account, often for
different users, and usually cares that each one lands rather than when.
The usual settings are:

- several keys in a `RotatingSigner`;
- a high `max_nonce_retries`, so contention never surfaces as an error;
- an early wait level such as `Included`, so a send returns once the
  transaction is in a block;
- a timeout around each send, since unlimited retries no longer bound the
  time.

```rust,no_run
use std::time::Duration;

use near_kit::protocol::{Action, SignedDelegateAction};
use near_kit::transaction::Included;
use near_kit::*;

# async fn example(relayer: &SigningNear, signed: SignedDelegateAction) -> Result<(), Box<dyn std::error::Error>> {
let relayer = relayer.clone().max_nonce_retries(u32::MAX).wait_until::<Included>();

let send = relayer
    .transaction(signed.sender_id())
    .add_action(Action::delegate(signed));
let response = tokio::time::timeout(Duration::from_secs(60), send).await??;
println!("relayed in {}", response.transaction_hash);
# Ok(())
# }
```

Track what you returned early on with
[`Near::tx_status`](crate::Near::tx_status) (see the
[finality chapter](crate::guide::finality#fire-and-track)). The
[meta-transactions chapter](crate::guide::meta_transactions) covers what a
relayer should check before paying for a user's actions.
