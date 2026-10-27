//! Crate-internal paths for the types that live in `near-kit-primitives`.
//!
//! The protocol types, keys, and views moved to `near-kit-primitives`; this
//! module keeps `crate::types::*` resolving for the client code. The public
//! paths are the facade modules (`protocol`, `rpc`, `signer`, `transaction`,
//! `standards`) and the crate-root re-exports.

// Most of these are used only by the `rpc`-gated client.
#![cfg_attr(not(feature = "rpc"), allow(unused_imports))]

pub(crate) use near_kit_primitives::protocol::*;
pub(crate) use near_kit_primitives::rpc::*;
pub(crate) use near_kit_primitives::signer::*;
pub(crate) use near_kit_primitives::standards::nep413;
pub(crate) use near_kit_primitives::transaction::*;
pub(crate) use near_kit_primitives::{AccountId, CryptoHash, Gas, NearToken};
