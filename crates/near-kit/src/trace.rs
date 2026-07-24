//! Internal facade over the `tracing` crate.
//!
//! With the `tracing` feature enabled (the default) this re-exports the real
//! macros and types. Without it, everything compiles down to no-ops so call
//! sites don't need `cfg` guards. The one thing this can't cover is the
//! `#[tracing::instrument]` attribute macro — those sites use
//! `#[cfg_attr(feature = "tracing", tracing::instrument(...))]` instead.
//!
//! Note the no-op event/span macros discard their arguments without
//! evaluating them, so values that are only used inside a tracing call must
//! themselves be gated on the feature (or they trigger unused warnings).

// Which re-exports are referenced depends on other features (e.g. `info` is
// only used under `sandbox`), and rustc's unused-import tracking is unreliable
// across macro re-export chains — so allow unused here.
#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub(crate) use tracing::{
    Instrument, Span, debug, debug_span, error, field, info, info_span, trace, warn,
};

// Only the event-macro stand-ins have users in every feature combination
// (e.g. `types/error.rs`); the span/instrument/field stand-ins are used
// exclusively from `rpc`-gated code, so without `rpc` they are dead code.
#[cfg(not(feature = "tracing"))]
#[cfg_attr(not(feature = "rpc"), allow(dead_code, unused_macros))]
mod noop {
    /// No-op stand-in for `tracing::Span`.
    #[derive(Clone, Debug)]
    pub(crate) struct Span;

    impl Span {
        pub(crate) fn current() -> Self {
            Span
        }

        pub(crate) fn record<Q: ?Sized, V>(&self, _field: &Q, _value: V) -> &Self {
            self
        }
    }

    /// No-op stand-in for `tracing::Instrument`: `.instrument(span)` returns
    /// the future unchanged.
    pub(crate) trait Instrument: Sized {
        fn instrument(self, _span: Span) -> Self {
            self
        }
    }

    impl<T> Instrument for T {}

    /// No-op stand-ins for the `tracing::field` helpers used in plain code
    /// (the ones inside span/event macros are discarded with the macro args).
    pub(crate) mod field {
        pub(crate) fn display<T>(value: T) -> T {
            value
        }
    }

    macro_rules! noop_event {
        ($($args:tt)*) => {{}};
    }

    macro_rules! noop_span {
        ($($args:tt)*) => {
            $crate::trace::Span
        };
    }

    pub(crate) use {noop_event, noop_span};
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub(crate) use noop::{
    Instrument, Span, field, noop_event as debug, noop_event as error, noop_event as info,
    noop_event as trace, noop_event as warn, noop_span as debug_span, noop_span as info_span,
};
