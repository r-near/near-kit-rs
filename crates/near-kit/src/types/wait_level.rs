//! Type-safe wait levels for transaction submission.
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. This lets the compiler determine the return type
//! of `send().wait_until(...)` based on which marker you pass in:
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # async fn example(near: &Near) -> Result<(), Error> {
//! // Default — returns FinalExecutionOutcome
//! near.transfer("bob.testnet", NearToken::from_near(1)).await?;
//!
//! // Executed levels — also returns FinalExecutionOutcome
//! near.transfer("bob.testnet", NearToken::from_near(1))
//!     .wait_until(Final)
//!     .await?;
//!
//! // Non-executed levels — returns SendTxResponse (hash + sender, plus any
//! // partial outcome the RPC has already produced)
//! let response = near.transfer("bob.testnet", NearToken::from_near(1))
//!     .wait_until(Included)
//!     .await?;
//! println!("tx hash: {}", response.transaction_hash);
//! # Ok(())
//! # }
//! ```

use crate::error::Error;
use crate::types::AccountId;

use super::block_reference::TxExecutionStatus;
use super::rpc::{FinalExecutionOutcome, RawTransactionResponse, SendTxResponse};

mod sealed {
    pub trait Sealed {}
}

/// Trait for type-safe transaction wait levels.
///
/// Each wait level is a zero-sized marker type that carries:
/// - The [`TxExecutionStatus`] to send to the RPC
/// - An associated [`Response`](WaitLevel::Response) type that determines
///   what `send().wait_until(...)` returns
///
/// This trait is sealed and cannot be implemented outside this crate.
pub trait WaitLevel: sealed::Sealed + Send + Sync + 'static {
    /// The type returned when awaiting a transaction with this wait level.
    type Response: Send + 'static;

    /// The RPC wait_until value.
    fn status() -> TxExecutionStatus;

    /// Convert a raw RPC response into the appropriate return type.
    ///
    /// For executed levels, this extracts the outcome and checks for
    /// `InvalidTxError`. For non-executed levels, this builds a
    /// [`SendTxResponse`] with the transaction hash, sender ID, and any partial
    /// execution outcome the RPC returned (present from `EXPERIMENTAL_tx_status`,
    /// absent from `send_tx`).
    #[doc(hidden)]
    fn convert(
        response: RawTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, Error>;
}

// =============================================================================
// Non-executed wait levels → SendTxResponse
// =============================================================================

/// Build a [`SendTxResponse`], carrying through any partial execution outcome.
///
/// The outcome is present when the response comes from `EXPERIMENTAL_tx_status`
/// (which returns `receipts_outcome`/`receipts` even at early wait levels) and
/// absent when it comes from `send_tx` (which returns no outcome at these
/// levels). The outcome is passed through as-is — not validated — so callers can
/// inspect partial progress; final-status validation happens at executed levels.
fn send_tx_response(response: RawTransactionResponse, sender_id: &AccountId) -> SendTxResponse {
    SendTxResponse {
        transaction_hash: response.transaction_hash,
        sender_id: sender_id.clone(),
        outcome: response.outcome,
    }
}

/// Don't wait, return immediately after the RPC accepts the transaction.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// [`Near::tx_status`](crate::Near::tx_status) and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
///
/// Named `Submitted` instead of `None` to avoid shadowing `Option::None`.
#[derive(Clone, Copy, Debug)]
pub struct Submitted;

impl sealed::Sealed for Submitted {}
impl WaitLevel for Submitted {
    type Response = SendTxResponse;
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::None
    }
    fn convert(
        response: RawTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
        Ok(send_tx_response(response, sender_id))
    }
}

/// Wait for the transaction to be included in a block.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// [`Near::tx_status`](crate::Near::tx_status) and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
#[derive(Clone, Copy, Debug)]
pub struct Included;

impl sealed::Sealed for Included {}
impl WaitLevel for Included {
    type Response = SendTxResponse;
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::Included
    }
    fn convert(
        response: RawTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
        Ok(send_tx_response(response, sender_id))
    }
}

/// Wait for the transaction's block to reach finality.
///
/// Returns [`SendTxResponse`]. Its `outcome` is populated when polled via
/// [`Near::tx_status`](crate::Near::tx_status) and the node already has receipt
/// data; it is [`None`] on the `send_tx` path.
#[derive(Clone, Copy, Debug)]
pub struct IncludedFinal;

impl sealed::Sealed for IncludedFinal {}
impl WaitLevel for IncludedFinal {
    type Response = SendTxResponse;
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::IncludedFinal
    }
    fn convert(
        response: RawTransactionResponse,
        sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
        Ok(send_tx_response(response, sender_id))
    }
}

// =============================================================================
// Executed wait levels → FinalExecutionOutcome
// =============================================================================

/// Extract and validate the execution outcome from a raw response.
fn extract_outcome(
    response: RawTransactionResponse,
    level: &str,
) -> Result<FinalExecutionOutcome, Error> {
    let outcome = response.outcome.ok_or_else(|| {
        Error::InvalidTransaction(format!(
            "RPC returned no execution outcome for transaction {} at wait level {}",
            response.transaction_hash, level,
        ))
    })?;

    use super::error::TxExecutionError;
    use super::rpc::FinalExecutionStatus;
    match outcome.status {
        FinalExecutionStatus::Failure(TxExecutionError::InvalidTxError(e)) => {
            Err(Error::InvalidTx(Box::new(e)))
        }
        _ => Ok(outcome),
    }
}

/// Wait for execution (optimistic, not yet finalized).
///
/// Returns [`FinalExecutionOutcome`]. This is the default when using
/// `.send().await` without specifying a wait level.
#[derive(Clone, Copy, Debug)]
pub struct ExecutedOptimistic;

impl sealed::Sealed for ExecutedOptimistic {}
impl WaitLevel for ExecutedOptimistic {
    type Response = FinalExecutionOutcome;
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::ExecutedOptimistic
    }
    fn convert(
        response: RawTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
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
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::Executed
    }
    fn convert(
        response: RawTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
        extract_outcome(response, "Executed")
    }
}

/// Wait for full finality (all receipts executed, all blocks finalized).
///
/// Returns [`FinalExecutionOutcome`].
#[derive(Clone, Copy, Debug)]
pub struct Final;

impl sealed::Sealed for Final {}
impl WaitLevel for Final {
    type Response = FinalExecutionOutcome;
    fn status() -> TxExecutionStatus {
        TxExecutionStatus::Final
    }
    fn convert(
        response: RawTransactionResponse,
        _sender_id: &AccountId,
    ) -> Result<Self::Response, Error> {
        extract_outcome(response, "Final")
    }
}
