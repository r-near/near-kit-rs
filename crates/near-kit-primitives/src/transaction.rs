//! Standalone function-call construction and typed execution wait levels.
//! The awaitable [`View`](crate::View) and [`Call`](crate::Call) values that
//! contract clients return are at the crate root.
//!
//! Sending a transaction needs a client (`near-kit`'s `SigningNear::transaction`);
//! to build and sign a transaction offline, use
//! [`Transaction`](crate::protocol::Transaction) directly.

mod function_call;
pub use crate::error::CodecError;
pub use function_call::FunctionCall;

pub use crate::types::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};
