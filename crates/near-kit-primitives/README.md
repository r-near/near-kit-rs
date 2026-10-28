# near-kit-primitives

NEAR Protocol types, keys, signing and transaction building, with no RPC
client, no async runtime and no HTTP stack.

[API docs](https://docs.rs/near-kit-primitives) · [near-kit](https://crates.io/crates/near-kit)

This is the offline core of [`near-kit`](https://crates.io/crates/near-kit).
near-kit depends on it and re-exports every item at the same path under
`near_kit::` (`near_kit_primitives::protocol::Action` is
`near_kit::protocol::Action`), so code using both sees one set of types.

Pick this crate over near-kit when you can't or don't want a network stack: a
smart contract that builds calls to other contracts, a signing service that
signs transactions built elsewhere, a tool that decodes RPC responses, or a
backend of your own. Otherwise use near-kit.

```sh
cargo add near-kit-primitives
cargo add near-kit-primitives --features contracts   # the #[contract] macro
```

## Modules

| Module | Contents |
|---|---|
| crate root | `AccountId`, `AccountIdRef`, `NearToken`, `Gas`, `CryptoHash`, `account!`; the verbs `Viewer` and `TxSender`; the awaitable `View` and `Call`, `ContractError`, `Contract`; `MaybeSend` and `MaybeSync` |
| `protocol` | Actions, transactions, delegate actions, chain IDs, state init, execution errors |
| `signer` | Keys, signatures, `Signer`, `InMemorySigner`, `RotatingSigner` |
| `transaction` | `FunctionCall`, `CodecError` and the typed wait levels |
| `rpc` | JSON-RPC response views, `BlockReference`, `Finality` |
| `standards` | NEP-413 message signing and verification |

## Signing a transaction offline

The nonce and block hash come from the chain, so fetch them wherever you have
network access and pass them in:

```rust
use near_kit_primitives::protocol::{Action, Transaction};
use near_kit_primitives::signer::SecretKey;
use near_kit_primitives::{CryptoHash, NearToken, account};

fn main() {
    let secret = SecretKey::generate_ed25519();
    let block_hash = CryptoHash::ZERO; // a recent block hash, fetched elsewhere

    let tx = Transaction::new(
        account!("alice.testnet"),
        secret.public_key(),
        1, // nonce: the access key's current nonce + 1
        account!("bob.testnet"),
        block_hash,
        vec![Action::transfer(NearToken::from_near(1))],
    );
    let signed = tx.sign(&secret);
    let base64 = signed.to_base64(); // what the `send_tx` RPC method takes
    assert!(!base64.is_empty());
}
```

## Calling other contracts from a near-sdk contract

With the `contracts` feature, `#[contract]` turns a trait into offline
constructors: `Counter::add(args)` returns a `FunctionCall` with the method
name, encoded arguments, gas (30 Tgas unless set) and deposit. Inside a
near-sdk contract, convert it with `FunctionCallAction::try_from` and pass its
public fields to `Promise::function_call`:

```rust
use near_kit_primitives::protocol::FunctionCallAction;
use near_kit_primitives::transaction::CodecError;
use near_kit_primitives::{Gas, NearToken};

#[near_kit_primitives::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn add(&mut self, args: AddArgs);
}

#[derive(serde::Serialize)]
pub struct AddArgs {
    pub value: u64,
}

fn main() -> Result<(), CodecError> {
    let call = FunctionCallAction::try_from(
        Counter::add(AddArgs { value: 5 })
            .gas(Gas::from_tgas(10))
            .deposit(NearToken::from_yoctonear(1)),
    )?;
    assert_eq!(call.method_name, "add");
    assert_eq!(call.args, br#"{"value":5}"#);
    // In the contract:
    // Promise::new(counter_id).function_call(call.method_name, call.args, call.deposit, call.gas)
    Ok(())
}
```

The conversion fails only if the arguments didn't serialize. A contract built
for `wasm32-unknown-unknown` also needs a `getrandom` backend, even though it
never generates keys. Set it for that target in `.cargo/config.toml` (a
`RUSTFLAGS` environment variable replaces this setting, so don't set both):

```toml
[target.wasm32-unknown-unknown]
rustflags = ['--cfg', 'getrandom_backend="unsupported"']
```

## Your own backend

Implement `Viewer` (`view_function`) or `TxSender` (`account_id` and `send`)
and `View`, `Call` and `#[contract]` clients run on it: a test double, a
gateway, or a sender that queues actions. near-kit's
[testing guide](https://docs.rs/near-kit/latest/near_kit/guide/testing/index.html)
covers mocks in full.

## Features

| Feature | Default | Enables |
|---|---|---|
| `contracts` | no | The `#[contract]` macro: offline `FunctionCall` constructors and a client generic over any `Viewer` / `TxSender` |
| `mnemonic` | no | BIP-39 seed phrases and SLIP-10 key derivation |
| `tracing` | no | `tracing` events (a WARN when an RPC error isn't recognized) |
| `js` | no | Entropy from the JS host (browser, Node, Deno) on `wasm32-unknown-unknown` |

near-kit's `mnemonic`, `js` and `tracing` features turn on the ones here.

Key generation and NEP-413 nonces use `getrandom`. On `wasm32-unknown-unknown`,
enable `js` inside a JS host, or pick a backend with
`--cfg getrandom_backend=".."` otherwise. WASI targets need neither. near-kit's
[platforms guide](https://docs.rs/near-kit/latest/near_kit/guide/platforms/index.html)
has the details.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
