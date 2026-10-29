//! # near-kit-primitives
//!
//! NEAR Protocol types, keys, signing and transaction building, with no RPC
//! client, no async runtime and no HTTP stack.
//!
//! This is the offline core of [`near-kit`](https://docs.rs/near-kit/0.30.0-rc.1).
//! near-kit depends on it and re-exports every item at the same path under
//! `near_kit::` (`near_kit_primitives::protocol::Action` is
//! `near_kit::protocol::Action`), so code using both sees one set of types.
//!
//! Pick this crate over near-kit when you can't or don't want a network
//! stack: a smart contract that builds calls to other contracts, a signing
//! service that signs transactions built elsewhere, a tool that decodes RPC
//! responses, or a backend of your own. Otherwise use near-kit.
//!
//! Chain work here is plain values ([`protocol::Action`],
//! [`transaction::FunctionCall`], [`rpc::BlockReference`]) plus two verbs:
//! [`Viewer`] runs a view function at a block and [`TxSender`] sends a
//! receiver and its actions. This crate defines the verbs and implements
//! neither; near-kit's `Near` and `SigningNear` do, and so can your own type.
//!
//! | Module | Contents |
//! |--------|----------|
//! | crate root | [`AccountId`], [`AccountIdRef`], [`NearToken`], [`Gas`], [`CryptoHash`], [`account!`]; the verbs [`Viewer`] and [`TxSender`]; the awaitable [`View`] and [`Call`], [`ContractError`], [`Contract`]; the platform bounds [`MaybeSend`] and [`MaybeSync`] |
//! | [`protocol`] | Actions, transactions, delegate actions, chain IDs, state init, execution errors |
//! | [`signer`] | Keys, signatures, [`signer::Signer`], [`signer::InMemorySigner`], [`signer::RotatingSigner`] |
//! | [`transaction`] | [`transaction::FunctionCall`], [`transaction::CodecError`] and the typed wait levels |
//! | [`rpc`] | JSON-RPC response views, [`rpc::BlockReference`], [`rpc::Finality`] |
//! | [`standards`] | NEP-413 message signing and verification |
//!
//! ## Signing a transaction offline
//!
//! The nonce and block hash come from the chain, so fetch them wherever you
//! have network access and pass them in:
//!
//! ```rust
//! use near_kit_primitives::protocol::{Action, Transaction};
//! use near_kit_primitives::signer::SecretKey;
//! use near_kit_primitives::{CryptoHash, NearToken, account};
//!
//! let secret = SecretKey::generate_ed25519();
//! let block_hash = CryptoHash::ZERO; // a recent block hash, fetched elsewhere
//!
//! let tx = Transaction::new(
//!     account!("alice.testnet"),
//!     secret.public_key(),
//!     1, // nonce: the access key's current nonce + 1
//!     account!("bob.testnet"),
//!     block_hash,
//!     vec![Action::transfer(NearToken::from_near(1))],
//! );
//! let signed = tx.sign(&secret);
//! let base64 = signed.to_base64(); // what the `send_tx` RPC method takes
//! assert!(!base64.is_empty());
//! ```
//!
//! ## Calling other contracts from a near-sdk contract
//!
//! With the `contracts` feature, `#[contract]` gives offline constructors
//! (`Counter::add(args)`) that return a [`transaction::FunctionCall`];
//! [`FunctionCallAction::try_from`](protocol::FunctionCallAction) splits it
//! into the parts near-sdk's `Promise::function_call` takes. near-kit's
//! [platforms guide](https://docs.rs/near-kit/0.30.0-rc.1/near_kit/guide/platforms/index.html#smart-contracts)
//! shows the conversion and the `getrandom` setting a contract needs.
//!
//! ## Your own backend
//!
//! Implement [`Viewer`] (`view_function`) or [`TxSender`] (`account_id` and
//! `send`) and [`View`], [`Call`] and `#[contract]` clients run on it:
//! a test double, a gateway, or a sender that queues actions. Both traits are
//! implemented for `&T`, `Box<T>` and `Arc<T>` too. The trait docs have
//! examples; near-kit's
//! [testing guide](https://docs.rs/near-kit/0.30.0-rc.1/near_kit/guide/testing/index.html)
//! covers mocks in full.
//!
//! ## Feature flags
//!
//! | Feature | Default | Enables |
//! |---------|---------|---------|
//! | `contracts` | no | The `#[contract]` macro: offline `FunctionCall` constructors and a client generic over any [`Viewer`] / [`TxSender`] |
//! | `mnemonic` | no | BIP-39 seed phrases and SLIP-10 key derivation |
//! | `tracing` | no | [`tracing`](https://docs.rs/tracing) events (a WARN when an RPC error isn't recognized) |
//! | `js` | no | Entropy from the JS host (browser, Node, Deno) on `wasm32-unknown-unknown` |
//!
//! near-kit's `mnemonic`, `js` and `tracing` features turn on the ones here.
//!
//! ## Entropy on WebAssembly
//!
//! Key generation and NEP-413 nonces use `getrandom`. On
//! `wasm32-unknown-unknown`, enable `js` inside a JS host, or pick a backend
//! with `--cfg getrandom_backend=".."` otherwise (`"custom"` for your own
//! source, `"unsupported"` when nothing needs randomness). WASI targets need
//! neither. near-kit's
//! [platforms guide](https://docs.rs/near-kit/0.30.0-rc.1/near_kit/guide/platforms/index.html)
//! has the details.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod backend;
mod error;
mod platform;
mod trace;
mod types;

pub mod protocol;
pub mod rpc;
pub mod signer;
pub mod standards;
pub mod transaction;

pub use backend::{Call, Contract, ContractError, TxSender, View, Viewer};
pub use platform::{MaybeSend, MaybeSync};
pub use types::{AccountId, AccountIdRef, CryptoHash, Gas, NearToken};

/// An [`AccountId`] from a string literal, checked at compile time.
///
/// Expands to `AccountId::from(const { AccountIdRef::new_or_panic("...") })`,
/// so an invalid literal is a build error rather than a runtime panic. Parse
/// runtime strings with [`str::parse`] instead.
///
/// ```rust
/// use near_kit_primitives::{AccountId, account};
///
/// let alice: AccountId = account!("alice.near");
/// assert_eq!(alice.as_str(), "alice.near");
/// ```
///
/// An invalid literal does not compile:
///
/// ```rust,compile_fail
/// let _ = near_kit_primitives::account!("Not An Account");
/// ```
#[macro_export]
macro_rules! account {
    ($account_id:literal) => {
        $crate::AccountId::from(const { $crate::AccountIdRef::new_or_panic($account_id) })
    };
}

// The typed-contract macro. Expansions name this crate when the caller
// doesn't depend on near-kit. Its `#[call]` / `#[json]` / `#[borsh]` markers
// are read by the macro itself and need no import.
#[cfg(feature = "contracts")]
pub use near_kit_macros::contract;

/// Support code shared with `near-kit` and the `#[contract]` expansion. Not
/// public API.
#[doc(hidden)]
pub mod __private {
    // One definition of the boxed future for both crates.
    pub use crate::platform::BoxFuture;

    // What `#[contract]` expansions call to build views and calls.
    pub use crate::backend::expansion::{borsh_args, call, json_args, view_borsh, view_json};

    // There is no client here, so the generated client's backend has no
    // default.
    pub use crate::__near_kit_primitives_client_struct as client_struct;
}

#[doc(hidden)]
#[macro_export]
macro_rules! __near_kit_primitives_client_struct {
    ($(#[$attr:meta])* $vis:vis struct $name:ident<$backend:ident> $body:tt) => {
        $(#[$attr])* $vis struct $name<$backend> $body
    };
}
