//! Internal facade over the `tracing` crate.
//!
//! With the opt-in `tracing` feature enabled, this re-exports the real event
//! macros. Without it, they compile down to no-ops that discard their
//! arguments without evaluating them, so call sites don't need `cfg` guards.
//! Only the macros this crate uses are provided; `near-kit` keeps its own,
//! larger copy for the RPC layer's spans.

#[cfg(feature = "tracing")]
pub(crate) use tracing::warn;

#[cfg(not(feature = "tracing"))]
macro_rules! noop_event {
    ($($args:tt)*) => {{}};
}

#[cfg(not(feature = "tracing"))]
pub(crate) use noop_event as warn;
