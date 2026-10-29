//! Type-safe wait levels for transaction submission.
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. In near-kit the level is a type parameter of the
//! signing client, `SigningNear<W>`, so the compiler knows what
//! `alice.wait_until::<W>().transaction(..).await` returns: executed levels a
//! [`FinalExecutionOutcome`], earlier ones a [`SendTxResponse`].
//!
//! ```rust
//! use near_kit_primitives::rpc::TxExecutionStatus;
//! use near_kit_primitives::transaction::{Final, Included, WaitLevel};
//!
//! // Each level carries the `wait_until` value sent to the RPC.
//! assert_eq!(Final::STATUS, TxExecutionStatus::Final);
//! assert_eq!(Included::STATUS, TxExecutionStatus::Included);
//! ```
//!
//! A `SigningNear` sends at [`Final`] unless `wait_until::<W>()` picks another
//! level. See near-kit's `transaction` module for the levels in use, including
//! polling an earlier send with `near.tx_status(..).wait_until::<W>()`.

use crate::error::OutcomeError;
use crate::types::AccountId;

use super::block_reference::TxExecutionStatus;
use super::error::TxExecutionError;
use super::rpc::{
    ExecutionStatus, FinalExecutionOutcome, FinalExecutionStatus, RpcTransactionResponse,
    SendTxResponse,
};

mod sealed {
    pub trait Sealed {}
}

/// Trait for type-safe transaction wait levels.
///
/// Each wait level is a zero-sized marker type that defines:
/// - The [`TxExecutionStatus`] to send to the RPC
/// - An associated [`Response`](WaitLevel::Response) type that determines
///   what a transaction sent at this level (`alice.wait_until::<W>()`)
///   returns
///
/// This trait is sealed and cannot be implemented outside this crate.
pub trait WaitLevel: sealed::Sealed + Send + Sync + 'static {
    /// The type returned when awaiting a transaction with this wait level.
    type Response: Send + 'static;

    /// The RPC `wait_until` value.
    const STATUS: TxExecutionStatus;

    /// Convert a raw RPC response into [`Response`](WaitLevel::Response).
    /// Executed levels reject an outcome with an `InvalidTxError`.
    #[doc(hidden)]
    fn convert(
        response: RpcTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError>;
}

/// Implements [`WaitLevel`] for a marker type: levels before execution return
/// a [`SendTxResponse`], with any outcome passed through unvalidated; executed
/// levels return the checked [`FinalExecutionOutcome`].
macro_rules! wait_level {
    ($level:ident, $status:ident, SendTxResponse) => {
        impl sealed::Sealed for $level {}
        impl WaitLevel for $level {
            type Response = SendTxResponse;
            const STATUS: TxExecutionStatus = TxExecutionStatus::$status;

            fn convert(
                response: RpcTransactionResponse,
                sender_id: &AccountId,
            ) -> Result<Self::Response, OutcomeError> {
                Ok(SendTxResponse {
                    transaction_hash: response.transaction_hash,
                    sender_id: sender_id.clone(),
                    outcome: response.outcome,
                })
            }
        }
    };
    ($level:ident, $status:ident, FinalExecutionOutcome) => {
        impl sealed::Sealed for $level {}
        impl WaitLevel for $level {
            type Response = FinalExecutionOutcome;
            const STATUS: TxExecutionStatus = TxExecutionStatus::$status;

            fn convert(
                response: RpcTransactionResponse,
                _sender_id: &AccountId,
            ) -> Result<Self::Response, OutcomeError> {
                extract_outcome(response, stringify!($level))
            }
        }
    };
}

/// Don't wait, return immediately after the RPC accepts the transaction.
///
/// Returns [`SendTxResponse`]; its [`outcome`](SendTxResponse::outcome) says
/// when one is present.
///
/// Named `Submitted` instead of `None` to avoid shadowing `Option::None`.
#[derive(Clone, Copy, Debug)]
pub struct Submitted;
wait_level!(Submitted, None, SendTxResponse);

/// Wait for the transaction to be included in a block.
///
/// Returns [`SendTxResponse`]; its [`outcome`](SendTxResponse::outcome) says
/// when one is present.
#[derive(Clone, Copy, Debug)]
pub struct Included;
wait_level!(Included, Included, SendTxResponse);

/// Wait for the transaction's block to reach finality.
///
/// Returns [`SendTxResponse`]; its [`outcome`](SendTxResponse::outcome) says
/// when one is present.
#[derive(Clone, Copy, Debug)]
pub struct IncludedFinal;
wait_level!(IncludedFinal, IncludedFinal, SendTxResponse);

/// Extract and validate the execution outcome from a raw response.
fn extract_outcome(
    response: RpcTransactionResponse,
    wait_level: &'static str,
) -> Result<FinalExecutionOutcome, OutcomeError> {
    let outcome = response.outcome.ok_or(OutcomeError::MissingOutcome {
        transaction_hash: response.transaction_hash,
        wait_level,
    })?;

    // nearcore derives the top-level status from the transaction outcome's,
    // so a rejection shows in both; check both.
    match (&outcome.status, &outcome.transaction_outcome.outcome.status) {
        (FinalExecutionStatus::Failure(TxExecutionError::InvalidTxError(e)), _)
        | (_, ExecutionStatus::InvalidTx(e)) => Err(OutcomeError::InvalidTx(Box::new(e.clone()))),
        _ => Ok(outcome),
    }
}

/// Wait for execution (optimistic, not yet finalized).
///
/// Returns [`FinalExecutionOutcome`]. Faster than [`Final`], but the blocks
/// may not be final yet, so a read at the default (final) block right after
/// can still miss the change. It is the RPC's own default `wait_until`;
/// near-kit's default is [`Final`].
#[derive(Clone, Copy, Debug)]
pub struct ExecutedOptimistic;
wait_level!(
    ExecutedOptimistic,
    ExecutedOptimistic,
    FinalExecutionOutcome
);

/// Wait for execution in a finalized block.
///
/// Returns [`FinalExecutionOutcome`].
#[derive(Clone, Copy, Debug)]
pub struct Executed;
wait_level!(Executed, Executed, FinalExecutionOutcome);

/// Wait for full finality (all receipts executed, all blocks finalized).
///
/// Returns [`FinalExecutionOutcome`]. This is the default level: a
/// `SigningNear` sends at `Final` unless `wait_until::<W>()` says otherwise,
/// so a read at the default finality right after sees the change.
#[derive(Clone, Copy, Debug)]
pub struct Final;
wait_level!(Final, Final, FinalExecutionOutcome);
