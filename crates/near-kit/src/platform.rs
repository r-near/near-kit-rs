//! Platform-conditional trait bounds for JS-host wasm compatibility.
//!
//! Defined once in near-kit-primitives (see its `platform` module) and
//! re-exported here. The items are `pub` because they appear in the public
//! [`RpcTransport`](crate::rpc::RpcTransport) interface — `BoxFuture` is
//! re-exported from there; `MaybeSend`/`MaybeSync` stay unnameable outside the
//! crate (their blanket impls apply to every eligible type, so implementors
//! never spell them out).

pub use near_kit_primitives::__private::{BoxFuture, MaybeSend, MaybeSync};
