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
//! | crate root | [`AccountId`], [`NearToken`], [`Gas`], [`CryptoHash`] |
//! | [`protocol`] | Actions, transactions, delegate actions, execution errors |
//! | [`signer`] | Keys, signatures, [`signer::Signer`], [`signer::InMemorySigner`], [`signer::RotatingSigner`] |
//! | [`transaction`] | [`transaction::FunctionCall`] and the typed wait levels |
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
//! | `js` | No | JS-host entropy backend (`getrandom`'s `wasm_js`) for `wasm32-unknown-unknown` |
//! | `tracing` | No | [`tracing`](https://docs.rs/tracing) events (a WARN for unrecognized RPC error variants) |
//!
//! Key generation and NEP-413 nonces use `getrandom`. On `wasm32-unknown-unknown`
//! enable `js` inside a JS host, or register a custom `getrandom` backend
//! otherwise; WASI targets need neither. See the `near-kit` crate docs for the
//! custom-backend setup.

mod error;
mod platform;
mod trace;
mod types;

pub mod protocol;
pub mod rpc;
pub mod signer;
pub mod standards;
pub mod transaction;

pub use types::{AccountId, CryptoHash, Gas, NearToken};
