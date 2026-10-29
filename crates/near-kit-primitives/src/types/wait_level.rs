//! Type-safe wait levels for transaction submission.
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. In near-kit the level is a type parameter of the
//! signing client, `SigningNear<W>`, so the compiler knows what
//! `alice.wait_until::<W>().transaction(..).await` returns:
//!
//! ```rust
//! use near_kit_primitives::rpc::TxExecutionStatus;
//! use near_kit_primitives::transaction::{Final, Included, WaitLevel};
//!
//! // Each level carries the `wait_until` value sent to the RPC...
//! assert_eq!(Final::STATUS, TxExecutionStatus::Final);
//! assert_eq!(Included::STATUS, TxExecutionStatus::Included);
//!
//! // ...and fixes the response type at compile time: executed levels return a
//! // `FinalExecutionOutcome`, earlier ones a `SendTxResponse`.
//! fn response_of<W: WaitLevel>(response: W::Response) -> W::Response {
//!     response
//! }
//! ```
//!
//! A `SigningNear` sends at [`Final`] unless `wait_until::<W>()` picks another
//! level. See near-kit's `transaction` module for the levels in use, including
//! polling an earlier send with `near.tx_status(..).wait_until::<W>()`.

use crate::error::OutcomeError;
use crate::types::AccountId;

use super::block_reference::TxExecutionStatus;
use super::rpc::{FinalExecutionOutcome, RpcTransactionResponse, SendTxResponse};

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

    /// Convert a raw RPC response into the appropriate return type.
    ///
    /// For executed levels, this extracts the outcome and checks for
    /// `InvalidTxError`. For non-executed levels, this builds a
    /// [`SendTxResponse`] with the transaction hash, sender ID, and any partial
    /// execution outcome the RPC returned (present from `EXPERIMENTAL_tx_status`,
    /// absent from `send_tx`).
    #[doc(hidden)]
    fn convert(
        response: RpcTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError>;
}

// =============================================================================
// Non-executed wait levels → SendTxResponse
// =============================================================================

/// Build a [`SendTxResponse`], carrying through any partial execution outcome.
///
/// The outcome is present once the node has one (from `IncludedFinal`, and
/// possibly at `Submitted` or `Included`; never when sending at `Submitted`),
/// whether the response comes from `send_tx` or `EXPERIMENTAL_tx_status`. It is passed through as-is — not validated — so
/// callers can inspect partial progress; final-status validation happens at
/// executed levels.
fn send_tx_response(response: RpcTransactionResponse, sender_id: &AccountId) -> SendTxResponse {
    SendTxResponse {
        transaction_hash: response.transaction_hash,
        sender_id: sender_id.clone(),
        outcome: response.outcome,
    }
}

/// Don't wait, return immediately after the RPC accepts the transaction.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// `Near::tx_status` and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
///
/// Named `Submitted` instead of `None` to avoid shadowing `Option::None`.
#[derive(Clone, Copy, Debug)]
pub struct Submitted;

impl sealed::Sealed for Submitted {}
impl WaitLevel for Submitted {
    type Response = SendTxResponse;
    const STATUS: TxExecutionStatus = TxExecutionStatus::None;

    fn convert(
        response: RpcTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        Ok(send_tx_response(response, sender_id))
    }
}

/// Wait for the transaction to be included in a block.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// `Near::tx_status` and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
#[derive(Clone, Copy, Debug)]
pub struct Included;

impl sealed::Sealed for Included {}
impl WaitLevel for Included {
    type Response = SendTxResponse;
    const STATUS: TxExecutionStatus = TxExecutionStatus::Included;

    fn convert(
        response: RpcTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        Ok(send_tx_response(response, sender_id))
    }
}

/// Wait for the transaction's block to reach finality.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// `Near::tx_status` and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
#[derive(Clone, Copy, Debug)]
pub struct IncludedFinal;

impl sealed::Sealed for IncludedFinal {}
impl WaitLevel for IncludedFinal {
    type Response = SendTxResponse;
    const STATUS: TxExecutionStatus = TxExecutionStatus::IncludedFinal;

    fn convert(
        response: RpcTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        Ok(send_tx_response(response, sender_id))
    }
}

// =============================================================================
// Executed wait levels → FinalExecutionOutcome
// =============================================================================

/// Extract and validate the execution outcome from a raw response.
fn extract_outcome(
    response: RpcTransactionResponse,
    wait_level: &'static str,
) -> Result<FinalExecutionOutcome, OutcomeError> {
    let outcome = response.outcome.ok_or(OutcomeError::MissingOutcome {
        transaction_hash: response.transaction_hash,
        wait_level,
    })?;

    use super::error::TxExecutionError;
    use super::rpc::{ExecutionStatus, FinalExecutionStatus};
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

impl sealed::Sealed for ExecutedOptimistic {}
impl WaitLevel for ExecutedOptimistic {
    type Response = FinalExecutionOutcome;
    const STATUS: TxExecutionStatus = TxExecutionStatus::ExecutedOptimistic;

    fn convert(
        response: RpcTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        extract_outcome(response, "ExecutedOptimistic")
    }
}

/// Wait for execution in a finalized block.
///
/// Returns [`FinalExecutionOutcome`].
#[derive(Clone, Copy, Debug)]
pub struct Executed;

impl sealed::Sealed for Executed {}
impl WaitLevel for Executed {
    type Response = FinalExecutionOutcome;
    const STATUS: TxExecutionStatus = TxExecutionStatus::Executed;

    fn convert(
        response: RpcTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        extract_outcome(response, "Executed")
    }
}

/// Wait for full finality (all receipts executed, all blocks finalized).
///
/// Returns [`FinalExecutionOutcome`]. This is the default level: a
/// `SigningNear` sends at `Final` unless `wait_until::<W>()` says otherwise,
/// so a read at the default finality right after sees the change.
#[derive(Clone, Copy, Debug)]
pub struct Final;

impl sealed::Sealed for Final {}
impl WaitLevel for Final {
    type Response = FinalExecutionOutcome;
    const STATUS: TxExecutionStatus = TxExecutionStatus::Final;

    fn convert(
        response: RpcTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, OutcomeError> {
        extract_outcome(response, "Final")
    }
}
