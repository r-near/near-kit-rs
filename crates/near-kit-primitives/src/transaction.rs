//! Standalone function-call construction, the awaitable [`View`] and
//! [`Call`] values that contract clients return, and typed execution wait
//! levels.
//!
//! Sending a transaction needs a client (`near-kit`'s `SigningNear::transaction`);
//! to build and sign a transaction offline, use
//! [`Transaction`](crate::protocol::Transaction) directly.

mod function_call;
pub use crate::backend::{Call, ContractError, View};
pub use crate::error::CodecError;
pub use function_call::FunctionCall;

pub use crate::types::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};
