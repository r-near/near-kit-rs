//! # near-kit-primitives
//!
//! NEAR Protocol types, keys, signing, and transaction construction with no
//! RPC client, no async runtime, and no HTTP stack.
//!
//! This is the offline core of [`near-kit`](https://docs.rs/near-kit). `near-kit`
//! depends on this crate and re-exports every item here at the same path under
//! `near_kit::` (for example `near_kit_primitives::protocol::Action` is
//! `near_kit::protocol::Action`), so the two can be mixed freely. Depend on this
//! crate directly when you only need the types: wasm contracts, signing services,
//! indexers, or any target without a network stack.
//!
//! | Module | Contents |
//! |--------|----------|
//! | crate root | [`AccountId`], [`NearToken`], [`Gas`], [`CryptoHash`], the chain verbs [`Viewer`] and [`TxSender`], [`account!`] |
//! | [`protocol`] | Actions, transactions, delegate actions, execution errors |
//! | [`signer`] | Keys, signatures, [`signer::Signer`], [`signer::InMemorySigner`], [`signer::RotatingSigner`] |
//! | [`transaction`] | [`transaction::FunctionCall`], the awaitable [`transaction::View`] / [`transaction::Call`], and the typed wait levels |
//! | [`rpc`] | JSON-RPC response views, [`rpc::BlockReference`], [`rpc::Finality`] |
//! | [`standards`] | NEP-413 message signing and verification |
//!
//! ## Building and signing a transaction offline
//!
//! ```rust
//! use near_kit_primitives::protocol::{Action, Transaction};
//! use near_kit_primitives::signer::SecretKey;
//! use near_kit_primitives::{CryptoHash, NearToken};
//!
//! let secret = SecretKey::generate_ed25519();
//! let block_hash = CryptoHash::ZERO; // a recent block hash, fetched elsewhere
//!
//! let tx = Transaction::new(
//!     "alice.testnet".parse()?,
//!     secret.public_key(),
//!     1, // nonce: the access key's current nonce + 1
//!     "bob.testnet".parse()?,
//!     block_hash,
//!     vec![Action::transfer(NearToken::from_near(1))],
//! );
//! let signed = tx.sign(&secret);
//! let bytes = signed.to_bytes(); // borsh, ready for `send_tx`
//! assert!(!bytes.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `mnemonic` | No | BIP-39 seed phrases and SLIP-10 hierarchical key derivation |
//! | `contracts` | No | The `#[contract]` macro: offline `FunctionCall` constructors plus a client generic over any [`Viewer`] / [`TxSender`] |
//! | `js` | No | JS-host entropy backend (`getrandom`'s `wasm_js`) for `wasm32-unknown-unknown` |
//! | `tracing` | No | [`tracing`](https://docs.rs/tracing) events (a WARN for unrecognized RPC error variants) |
//!
//! Key generation and NEP-413 nonces use `getrandom`. On `wasm32-unknown-unknown`
//! enable `js` inside a JS host, or register a custom `getrandom` backend
//! otherwise; WASI targets need neither. See the `near-kit` crate docs for the
//! custom-backend setup.

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

pub use backend::{Contract, TxSender, Viewer};
pub use types::{AccountId, CryptoHash, Gas, NearToken};

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
        $crate::AccountId::from(const { $crate::protocol::AccountIdRef::new_or_panic($account_id) })
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
    // One definition of the platform bounds for both crates, so a
    // `MaybeSend` bound written in near-kit and one written here are the
    // same trait.
    pub use crate::platform::{BoxFuture, MaybeSend, MaybeSync};

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
