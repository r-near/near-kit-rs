//! Compile-time examples for the type-only transaction wait-level API.

use near_kit::*;

#[test]
fn transaction_builders_select_wait_levels_in_type_position() {
    let near = Near::testnet().build();

    let default: TransactionSend<ExecutedOptimistic> =
        near.transfer("bob.testnet", NearToken::from_near(1)).send();
    let _included: TransactionSend<Included> = default.wait_until::<Included>();

    let _final_send: TransactionSend<Final> = near
        .transfer("bob.testnet", NearToken::from_near(1))
        .wait_until::<Final>();
}

#[test]
fn wait_level_types_map_to_rpc_statuses() {
    assert_eq!(Submitted::STATUS, TxExecutionStatus::None);
    assert_eq!(Included::STATUS, TxExecutionStatus::Included);
    assert_eq!(IncludedFinal::STATUS, TxExecutionStatus::IncludedFinal);
    assert_eq!(
        ExecutedOptimistic::STATUS,
        TxExecutionStatus::ExecutedOptimistic,
    );
    assert_eq!(Executed::STATUS, TxExecutionStatus::Executed);
    assert_eq!(Final::STATUS, TxExecutionStatus::Final);
}

#[tokio::test]
async fn status_query_reports_invalid_sender_when_awaited() {
    let near = Near::testnet().build();
    let error = near
        .tx_status(&CryptoHash::ZERO, "not a valid account ID")
        .await
        .unwrap_err();

    assert!(matches!(error, Error::ParseAccountId(_)));
}

// These helpers are intentionally compile-only. Their signatures document and
// verify the response type selected by each default and generic wait level.

#[allow(dead_code)]
async fn send_transaction_at<W: WaitLevel>(
    transaction: TransactionBuilder,
) -> Result<W::Response, Error> {
    transaction.wait_until::<W>().await
}

#[allow(dead_code)]
async fn send_call_at<W: WaitLevel>(call: CallBuilder) -> Result<W::Response, Error> {
    call.wait_until::<W>().await
}

#[allow(dead_code)]
async fn included_transaction_returns_progress(
    transaction: TransactionBuilder,
) -> Result<SendTxResponse, Error> {
    transaction.wait_until::<Included>().await
}

#[allow(dead_code)]
async fn final_transaction_returns_execution_outcome(
    transaction: TransactionBuilder,
) -> Result<FinalExecutionOutcome, Error> {
    transaction.wait_until::<Final>().await
}

#[allow(dead_code)]
async fn default_transaction_send_returns_execution_outcome(
    transaction: TransactionBuilder,
) -> Result<FinalExecutionOutcome, Error> {
    transaction.await
}

#[allow(dead_code)]
async fn send_signed_at<W: WaitLevel>(
    near: &Near,
    signed_tx: &SignedTransaction,
) -> Result<W::Response, Error> {
    near.send(signed_tx).wait_until::<W>().await
}

#[allow(dead_code)]
async fn query_status_at<W: WaitLevel>(
    near: &Near,
    tx_hash: &CryptoHash,
    sender_id: &AccountId,
) -> Result<W::Response, Error> {
    near.tx_status(tx_hash, sender_id).wait_until::<W>().await
}

#[allow(dead_code)]
async fn default_signed_send_returns_execution_outcome(
    near: &Near,
    signed_tx: &SignedTransaction,
) -> Result<FinalExecutionOutcome, Error> {
    near.send(signed_tx).await
}

#[allow(dead_code)]
async fn default_status_query_returns_current_progress(
    near: &Near,
    tx_hash: &CryptoHash,
    sender_id: &AccountId,
) -> Result<SendTxResponse, Error> {
    near.tx_status(tx_hash, sender_id).await
}
