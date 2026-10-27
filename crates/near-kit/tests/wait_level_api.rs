//! Compile-time examples for the type-only transaction wait-level API.
#![cfg(feature = "rpc")]

use near_kit::protocol::{Action, SignedTransaction};
use near_kit::rpc::{
    FinalExecutionOutcome, RpcClient, RpcError, SendTxResponse, TxExecutionStatus,
};
use near_kit::signer::InMemorySigner;
use near_kit::transaction::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, Tx, WaitLevel,
};
use near_kit::*;

fn alice() -> SigningNear {
    Near::testnet().build().with_signer(
        InMemorySigner::new(
            account!("alice.testnet"),
            "ed25519:3JoAjwLppjgvxkk6kNsu5wQj3FfUJnpBKWieC73hVTpBeA6FZiCc5tfyZL3a3tHeQJegQe4qGSv8FLsYp7TYd1r6",
        )
        .unwrap(),
    )
}

#[test]
fn signing_clients_select_wait_levels_in_type_position() {
    let alice = alice();
    let transfer = || Action::transfer(NearToken::from_near(1));

    let _default: Tx<Final> = alice
        .transaction(account!("bob.testnet"))
        .add_action(transfer());

    let included: SigningNear<Included> = alice.wait_until::<Included>();
    let _included: Tx<Included> = included
        .transaction(account!("bob.testnet"))
        .add_action(transfer());

    let _final: Tx<Final> = included
        .wait_until::<Final>()
        .transaction(account!("bob.testnet"))
        .add_action(transfer());
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

// These helpers are intentionally compile-only. Their signatures document and
// verify the response type selected by each default and generic wait level.

#[allow(dead_code)]
#[allow(clippy::result_large_err)] // Compile-check the public, unboxed RPC result.
async fn generic_rpc_call_remains_public(client: &RpcClient) -> Result<(), RpcError> {
    let _: serde_json::Value = client.call("status", serde_json::json!({})).await?;
    Ok(())
}

#[allow(dead_code)]
async fn send_transaction_at<W: WaitLevel>(
    sender: &SigningNear<W>,
    actions: Vec<Action>,
) -> Result<W::Response, Error> {
    TxSender::send(sender, account!("bob.testnet"), actions).await
}

#[allow(dead_code)]
async fn send_tx_at<W: WaitLevel>(transaction: Tx<W>) -> Result<W::Response, Error> {
    transaction.await
}

#[allow(dead_code)]
async fn included_transaction_returns_progress(
    sender: &SigningNear,
    actions: Vec<Action>,
) -> Result<SendTxResponse, Error> {
    sender
        .wait_until::<Included>()
        .send(account!("bob.testnet"), actions)
        .await
}

#[allow(dead_code)]
async fn final_transaction_returns_execution_outcome(
    transaction: Tx<Final>,
) -> Result<FinalExecutionOutcome, Error> {
    transaction.await
}

#[allow(dead_code)]
async fn default_transaction_send_returns_execution_outcome(
    transaction: Tx,
) -> Result<FinalExecutionOutcome, Error> {
    transaction.await
}

#[allow(dead_code)]
async fn send_signed_at<W: WaitLevel>(
    near: &Near,
    signed_tx: &SignedTransaction,
) -> Result<W::Response, Error> {
    near.send_signed(signed_tx).wait_until::<W>().await
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
    near.send_signed(signed_tx).await
}

#[allow(dead_code)]
async fn default_status_query_returns_current_progress(
    near: &Near,
    tx_hash: &CryptoHash,
    sender_id: &AccountId,
) -> Result<SendTxResponse, Error> {
    near.tx_status(tx_hash, sender_id).await
}
