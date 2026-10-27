//! Standalone function-call construction and typed execution wait levels.
//!
//! The fluent `TransactionBuilder` needs a client and lives in `near-kit`;
//! to build and sign a transaction offline, use
//! [`Transaction`](crate::protocol::Transaction) directly.

mod function_call;
pub use crate::error::CodecError;
pub use function_call::FunctionCall;

pub use crate::types::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};
