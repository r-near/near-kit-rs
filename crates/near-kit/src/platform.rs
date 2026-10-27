//! Platform-conditional trait bounds for JS-host wasm compatibility.
//!
//! Defined once in near-kit-primitives (see its `platform` module) and
//! re-exported here. `BoxFuture` is public through
//! [`rpc`](crate::rpc) because it appears in the
//! [`RpcTransport`](crate::rpc::RpcTransport) interface; `MaybeSend` and
//! `MaybeSync` are public at the crate root.

pub use near_kit_primitives::__private::BoxFuture;
pub use near_kit_primitives::{MaybeSend, MaybeSync};
