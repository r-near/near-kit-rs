# Offline signing

A NEAR transaction needs two values from the chain before it can be
signed: a nonce for the access key, and the hash of a recent block. Every
path in this chapter is about where those two values come from.

## Sign now, send later

[`SigningNear::sign`](crate::SigningNear::sign) fetches the nonce and block
hash, signs, and returns a [`SignedTransaction`](crate::protocol::SignedTransaction)
without sending it. Submit it with
[`Near::send_signed`](crate::Near::send_signed), from this process or any
other:

```rust,no_run
use near_kit::protocol::{Action, SignedTransaction};
use near_kit::*;

# async fn example(alice: &SigningNear, near: &Near) -> Result<(), Error> {
let signed = alice
    .sign(account!("bob.testnet"), vec![Action::transfer(NearToken::from_near(1))])
    .await?;
println!("will have hash {}", signed.get_hash());

// Base64 of the Borsh bytes: the form `send_tx` takes and wallets exchange.
let wire = signed.to_base64();

// ...elsewhere...
let signed = SignedTransaction::from_base64(&wire)?;
near.send_signed(&signed).await?.result()?;
# Ok(())
# }
```

`send_signed` waits for `Final` unless you pick a level with
`.wait_until::<W>()`. It borrows the transaction; to spawn the send, move
the transaction into an `async move` block.

Three things differ from sending a [`Tx`](crate::transaction::Tx):

- **No nonce retry.** The signature fixes the nonce. If another transaction
  from the same key lands first with a higher nonce, this one fails with
  `Error::InvalidTx(InvalidNonce)`, and you sign again.
- **Order matters.** `sign` takes the next nonce from the process-wide
  cache, so transactions signed one after another get increasing nonces.
  Send them in the order you signed them; sending a later one first makes
  the earlier ones invalid.
- **Expiry.** The block hash must be recent when the transaction reaches
  the chain. Once its block is older than the chain's transaction validity
  period (86,400 blocks on mainnet), the node rejects it with `Expired`.

## Fully offline

To sign on a machine with no network access, fetch the nonce and block
hash on a connected machine, carry them over, and build the
[`Transaction`](crate::protocol::Transaction) yourself.
[`view_access_key`](crate::rpc::RpcClient::view_access_key) returns both:
the key's current nonce, and the hash of the block it was read at.

```rust,no_run
use near_kit::rpc::Finality;
use near_kit::signer::PublicKey;
use near_kit::*;

// On the connected machine.
# async fn fetch(near: &Near, public_key: PublicKey) -> Result<(), Error> {
let key = near
    .rpc()
    .view_access_key(&account!("alice.testnet"), &public_key, Finality::Final)
    .await?;
let (nonce, block_hash) = (key.nonce + 1, key.block_hash);
# Ok(())
# }
```

```rust
use near_kit::protocol::{Action, SignedTransaction, Transaction};
use near_kit::signer::SecretKey;
use near_kit::*;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let secret_key = SecretKey::generate_ed25519();
# let (nonce, block_hash) = (42, CryptoHash::ZERO);
// On the offline machine: no client, no network.
let tx = Transaction::new(
    account!("alice.testnet"),
    secret_key.public_key(),
    nonce,
    account!("bob.testnet"),
    block_hash,
    vec![Action::transfer(NearToken::from_near(1))],
);
let wire = tx.sign(&secret_key).to_base64();

// Back on the connected machine, `SignedTransaction::from_base64(&wire)?`
// and `near.send_signed(&signed)`. Decoding checks nothing but the encoding:
let signed = SignedTransaction::from_base64(&wire)?;
assert_eq!(signed.transaction.receiver_id, "bob.testnet");
# Ok(())
# }
```

The same code runs without near-kit's RPC layer at all, with
`default-features = false` or with near-kit-primitives on its own; see the
[platforms chapter](crate::guide::platforms#offline-builds).

Any [`FunctionCall`](crate::transaction::FunctionCall) becomes an action
with [`into_action`](crate::transaction::FunctionCall::into_action), which
fails only if its arguments failed to encode:

```rust
use near_kit::transaction::FunctionCall;
use near_kit::*;

let call = FunctionCall::new("add_message")
    .args(serde_json::json!({ "text": "signed offline" }))
    .gas(Gas::from_tgas(30))
    .into_action()?;
# Ok::<(), near_kit::transaction::CodecError>(())
```

## Keys you can't export

`Transaction::sign` needs a [`SecretKey`](crate::signer::SecretKey). For a
hardware wallet, an HSM or a remote signing service, sign the transaction
hash and attach the signature with
[`into_signed`](crate::protocol::Transaction::into_signed). Through a
[`Signer`](crate::signer::Signer), claim the key once, since a rotating
signer hands out a different key on each call:

```rust,no_run
use near_kit::protocol::{Action, SignedTransaction, Transaction};
use near_kit::signer::Signer;
use near_kit::*;

async fn sign_with(
    signer: &dyn Signer,
    nonce: u64,
    block_hash: CryptoHash,
) -> Result<SignedTransaction, Error> {
    let key = signer.key();
    let tx = Transaction::new(
        signer.account_id().clone(),
        key.public_key().clone(),
        nonce,
        account!("bob.testnet"),
        block_hash,
        vec![Action::transfer(NearToken::from_near(1))],
    );
    let signature = key.sign(tx.get_hash().as_bytes()).await?;
    Ok(tx.into_signed(signature))
}
```

The [keys chapter](crate::guide::keys) shows how to wrap a remote signer as
a `Signer` with [`SigningKey::from_backend`](crate::signer::SigningKey::from_backend),
so that `SigningNear` uses it for ordinary sends too.

Delegate actions work the same way: build a
[`DelegateAction`](crate::protocol::DelegateAction), sign
[`get_hash`](crate::protocol::DelegateAction::get_hash), and call
[`into_signed`](crate::protocol::DelegateAction::into_signed).
[`SigningNear::sign_delegate`](crate::SigningNear::sign_delegate) also
makes no RPC calls when its `DelegateOptions` set both `nonce` and
`max_block_height`.

## Signing messages (NEP-413)

NEP-413 signs a message instead of a transaction, for logging in or proving
that an account holds a key. Nothing goes on chain and no gas is spent. The
signer signs [`SignMessageParams`](crate::standards::nep413::SignMessageParams)
(the message, the recipient app, a 32-byte nonce) with
[`SigningKey::sign_nep413`](crate::signer::SigningKey::sign_nep413):

```rust
use near_kit::signer::{InMemorySigner, SecretKey, Signer};
use near_kit::standards::nep413::{self, AuthPayload, NonceValidation};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let alice = InMemorySigner::implicit(SecretKey::generate_ed25519())?;

// The app asks the user to sign.
let params = nep413::SignMessageParams {
    message: "Log in to example.app".to_string(),
    recipient: "example.app".to_string(),
    nonce: nep413::generate_nonce(),
    callback_url: None,
    state: None,
};
let signed = alice.key().sign_nep413(alice.account_id(), &params).await?;

// The JSON a frontend posts to its backend, with camelCase field names.
let body = serde_json::to_string(&AuthPayload::from_signed(signed, &params))?;

// The backend checks that the payload was signed for this app, then the
// signature and that the nonce is under 5 minutes old.
let payload: AuthPayload = serde_json::from_str(&body)?;
assert_eq!(payload.recipient, "example.app");
assert_eq!(payload.message, "Log in to example.app");
assert!(nep413::verify_signature(
    &payload.signed_message,
    &payload.to_params(),
    NonceValidation::default(),
));
# Ok(())
# }
```

From a `SigningNear`, sign with `alice.signer().key()` and
`alice.account_id()`; there is no separate method.

[`verify_signature`](crate::standards::nep413::verify_signature) checks
the signature and the nonce's age only. It doesn't check that the public
key belongs to the account the message names: anyone can sign a message
claiming to be `alice.near` with a key of their own. To prove the account,
use [`nep413::verify`](crate::standards::nep413::verify). With the default
[`VerifyOptions`](crate::standards::nep413::VerifyOptions) it also looks the
key up on chain and requires it to be a full-access key of that account.

Neither function checks what the message was signed for. The payload's
recipient, message and nonce come from the request, so a login handler has
to compare them with what the app asked for: otherwise a signature the user
made for another app, or a replay of an earlier login, gets in. Here the
server generated each nonce with
[`generate_nonce`](crate::standards::nep413::generate_nonce), kept it in
`issued` and handed it to the frontend; each is accepted once:

```rust,no_run
use std::collections::HashSet;

use near_kit::standards::nep413::{self, AuthPayload};
use near_kit::*;

# async fn example(
#     near: &Near,
#     payload: AuthPayload,
#     issued: &mut HashSet<[u8; 32]>,
# ) -> Result<AccountId, Error> {
let rejected = || Error::Config("login rejected".into());
if payload.recipient != "example.app"
    || payload.message != "Log in to example.app"
    || !issued.contains(&payload.nonce)
{
    return Err(rejected());
}
let params = payload.to_params();
if !nep413::verify(&payload.signed_message, &params, near, Default::default()).await? {
    return Err(rejected());
}
issued.remove(&payload.nonce);
Ok(payload.signed_message.account_id.clone())
# }
```

[`generate_nonce`](crate::standards::nep413::generate_nonce) puts a
millisecond timestamp in the first 8 bytes of the nonce, and the default
[`NonceValidation`](crate::standards::nep413::NonceValidation) rejects
signatures older than five minutes or from the future. That layout is a
near-kit convention; NEP-413 itself treats the nonce as opaque. For apps
with their own nonce scheme, pass `NonceValidation::None` and handle replay
protection yourself, for example by remembering nonces you have accepted.
