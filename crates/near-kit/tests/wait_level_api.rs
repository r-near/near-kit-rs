//! Compile-time examples for the type-only transaction wait-level API.
#![cfg(feature = "rpc")]

use std::sync::Arc;

use near_kit::protocol::{ChainId, SignedTransaction};
use near_kit::rpc::{
    FinalExecutionOutcome, RpcClient, RpcError, SandboxNetwork, SendTxResponse, TxExecutionStatus,
};
use near_kit::signer::{InMemorySigner, Signer};
use near_kit::transaction::{
    CallBuilder, Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted,
    TransactionBuilder, TransactionSend, WaitLevel,
};
use near_kit::*;

struct ExternalSandbox {
    chain_id: ChainId,
    root_signer: Arc<dyn Signer>,
}

impl SandboxNetwork for ExternalSandbox {
    fn rpc_url(&self) -> &str {
        "http://127.0.0.1:3030"
    }

    fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    fn root_signer(&self) -> Arc<dyn Signer> {
        self.root_signer.clone()
    }
}

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
fn sandbox_network_is_implementable_without_the_lifecycle_crate() {
    let sandbox = ExternalSandbox {
        chain_id: ChainId::new("sandbox"),
        root_signer: Arc::new(
            InMemorySigner::new(
                "sandbox",
                "ed25519:3JoAjwLppjgvxkk6kNsu5wQj3FfUJnpBKWieC73hVTpBeA6FZiCc5tfyZL3a3tHeQJegQe4qGSv8FLsYp7TYd1r6",
            )
            .unwrap(),
        ),
    };
    let near = Near::sandbox(&sandbox);
    assert_eq!(near.rpc_url(), "http://127.0.0.1:3030");
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
async fn generic_rpc_call_remains_public(client: &RpcClient) -> Result<(), RpcError> {
    let _: serde_json::Value = client.call("status", serde_json::json!({})).await?;
    Ok(())
}

#[allow(dead_code)]
async fn send_transaction_at<W: WaitLevel>(
    transaction: TransactionBuilder,
) -> Result<W::Response, Error> {
    transaction.wait_until::<W>().await
}

#[allow(dead_code)]
async fn send_call_at<W: WaitLevel>(call: CallBuilder) -> Result<W::Response, Error> {
    call.send().wait_until::<W>().await
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
