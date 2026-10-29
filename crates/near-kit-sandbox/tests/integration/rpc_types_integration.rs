//! Integration tests for RPC types against near-sandbox.
//!
//! These tests verify that all RPC response types correctly deserialize
//! from real NEAR RPC responses.

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{rpc::*, signer::SecretKey, transaction::*};
use near_kit_sandbox::SANDBOX_ROOT_ACCOUNT;

use super::support::unique_account;

#[tokio::test]
async fn test_block_view_full_fields() {
    let (_, near) = super::support::shared_client().await;

    let block = near
        .near()
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .unwrap();

    assert!(
        !block.author.as_str().is_empty(),
        "Block should have an author"
    );

    // The genesis block may have height 0
    let header = &block.header;
    assert!(!header.hash.is_zero(), "Block hash should not be zero");
    assert!(
        !header.timestamp_nanosec.is_empty(),
        "Timestamp nanosec should exist"
    );
    assert!(
        header.gas_price.as_yoctonear() > 0,
        "Gas price should exist"
    );
    assert!(
        header.total_supply.as_yoctonear() > 0,
        "Total supply should exist"
    );
    assert!(
        !header.signature.as_bytes().is_empty(),
        "Signature should exist"
    );
    assert!(
        header.latest_protocol_version > 0,
        "Protocol version should be positive"
    );

    for chunk in &block.chunks {
        assert!(!chunk.chunk_hash.is_zero(), "Chunk hash should not be zero");
        assert!(chunk.gas_limit > 0, "Gas limit should be positive");
        assert!(
            !chunk.signature.as_bytes().is_empty(),
            "Chunk signature should exist"
        );
    }
}

#[tokio::test]
async fn test_status_response_full_fields() {
    let (_, near) = super::support::shared_client().await;

    let status = near.near().rpc().status().await.unwrap();

    assert!(
        status.protocol_version > 0,
        "Protocol version should be positive"
    );
    assert!(
        status.latest_protocol_version >= status.protocol_version,
        "Latest protocol version should be >= current"
    );
    assert!(!status.chain_id.is_empty(), "Chain ID should exist");
    assert!(!status.genesis_hash.is_zero(), "Genesis hash should exist");

    // SyncInfo - note latest_block_height may be 0 for genesis
    assert!(
        !status.sync_info.latest_block_hash.is_zero(),
        "Latest block hash should exist"
    );
    assert!(
        !status.sync_info.latest_block_time.is_empty(),
        "Latest block time should exist"
    );

    assert!(!status.version.version.is_empty(), "Version should exist");
    assert!(!status.version.build.is_empty(), "Build should exist");
}

#[tokio::test]
async fn test_account_view_full_fields() {
    let (_, near) = super::support::shared_client().await;

    let account_id: AccountId = SANDBOX_ROOT_ACCOUNT.into();
    let account = near
        .near()
        .rpc()
        .view_account(&account_id, near_kit::rpc::Finality::Final)
        .await
        .unwrap();

    assert!(
        account.amount.as_yoctonear() > 0,
        "Account should have balance"
    );
    assert!(account.storage_usage > 0, "Account should use some storage");
    // block_height may be 0 for genesis block
    assert!(!account.block_hash.is_zero(), "Block hash should exist");
}

#[tokio::test]
async fn test_access_key_list_view() {
    let (_, near) = super::support::shared_client().await;

    let account_id: AccountId = SANDBOX_ROOT_ACCOUNT.into();
    let keys = near
        .near()
        .rpc()
        .view_access_key_list(&account_id, near_kit::rpc::Finality::Final)
        .await
        .unwrap();

    assert!(
        !keys.keys.is_empty(),
        "Account should have at least one key"
    );
    // block_height may be 0 for genesis block
    assert!(!keys.block_hash.is_zero(), "Block hash should exist");
}

#[tokio::test]
async fn test_final_execution_outcome_full_fields() {
    let (_, near) = super::support::shared_client().await;

    let root_account: AccountId = SANDBOX_ROOT_ACCOUNT.into();

    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account("rpc");

    let outcome = near
        .wait_until::<Final>()
        .transaction(&receiver_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(5)))
        .add_action(Action::add_full_access_key(receiver_key.public_key()))
        .await
        .unwrap();

    assert!(outcome.is_success(), "Transaction should succeed");
    assert!(
        !outcome.receipts_outcome.is_empty(),
        "Should have receipt outcomes"
    );

    let tx = &outcome.transaction;
    assert_eq!(tx.signer_id, root_account);
    assert_eq!(tx.receiver_id, receiver_id);
    assert!(tx.nonce > 0, "Nonce should be positive");
    assert!(!tx.hash.is_zero(), "Transaction hash should exist");
    assert!(
        matches!(
            &tx.actions[..],
            [
                ActionView::CreateAccount,
                ActionView::Transfer { .. },
                ActionView::AddKey { .. }
            ]
        ),
        "unexpected actions: {:?}",
        tx.actions
    );

    let tx_outcome = &outcome.transaction_outcome;
    assert!(!tx_outcome.id.is_zero(), "Outcome ID should exist");
    assert!(!tx_outcome.block_hash.is_zero(), "Block hash should exist");
    assert_eq!(tx_outcome.outcome.executor_id, root_account);
    assert!(tx_outcome.outcome.gas_burnt.as_gas() > 0, "Should burn gas");
    assert!(
        !tx_outcome.outcome.receipt_ids.is_empty(),
        "Should generate receipts"
    );

    for receipt_outcome in &outcome.receipts_outcome {
        assert!(!receipt_outcome.id.is_zero(), "Receipt ID should exist");
        assert!(
            !receipt_outcome.block_hash.is_zero(),
            "Block hash should exist"
        );
    }
}

/// A status lookup of a hash the node has never seen is a typed,
/// non-retryable error at the default (`Submitted`) level. That it is sent
/// only once is checked against a counting transport, in near-kit's
/// `test_deterministic_lookup_errors_are_not_retried`.
#[tokio::test]
async fn test_tx_status_unknown_transaction() {
    let (_, near) = super::support::shared_client().await;
    let unknown = CryptoHash::hash(b"never sent");

    let err = near
        .near()
        .tx_status(&unknown, SANDBOX_ROOT_ACCOUNT)
        .await
        .unwrap_err();

    match &err {
        Error::Rpc(rpc) => {
            assert!(
                matches!(**rpc, RpcError::TransactionNotFound(hash) if hash == unknown),
                "{rpc:?}"
            );
            assert!(!rpc.is_retryable());
        }
        other => panic!("expected Error::Rpc, got {other:?}"),
    }
}

/// At an early wait level, `tx_status` must surface the partial execution
/// outcome (receipts_outcome/receipts) instead of dropping it.
///
/// `EXPERIMENTAL_tx_status` returns receipt data even at `Included`/`Submitted`
/// (the wait level only controls how long the server blocks, not whether it
/// returns an outcome), so a frontend can poll here to drive a progressive UI.
/// The transaction is settled first so the outcome is deterministically present.
#[tokio::test]
async fn test_tx_status_early_wait_level_surfaces_receipts() {
    let (_, near) = super::support::shared_client().await;

    let root_account: AccountId = SANDBOX_ROOT_ACCOUNT.into();

    // Execute a transaction and let it settle.
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account("rpc");

    let outcome = near
        .wait_until::<Final>()
        .transaction(&receiver_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(2)))
        .add_action(Action::add_full_access_key(receiver_key.public_key()))
        .await
        .unwrap();

    let tx_hash = outcome.transaction_hash();

    // A bare status query defaults to Submitted: it returns the node's current
    // progress immediately instead of waiting for a later milestone.
    let current: SendTxResponse = near.near().tx_status(tx_hash, &root_account).await.unwrap();
    assert_eq!(&current.transaction_hash, tx_hash);
    assert_eq!(&current.sender_id, &root_account);
    assert!(
        current.outcome.is_some(),
        "default tx_status should surface the settled transaction's progress"
    );

    // Poll at an early wait level (Included). Previously this dropped the
    // outcome and returned only hash + sender; now it carries the partial
    // outcome so the receipts are reachable.
    let response: SendTxResponse = near
        .near()
        .tx_status(tx_hash, &root_account)
        .wait_until::<Included>()
        .await
        .unwrap();

    assert_eq!(&response.transaction_hash, tx_hash);
    assert_eq!(response.sender_id, root_account);

    let partial = response
        .outcome
        .expect("tx_status at Included should surface the partial outcome, not drop it");

    assert!(
        !partial.receipts_outcome.is_empty(),
        "early-level tx_status should expose receipts_outcome for progressive UI"
    );
    // EXPERIMENTAL_tx_status also populates full receipt details.
    assert!(
        !partial.receipts.is_empty(),
        "early-level tx_status should expose receipts (receiver_id -> stage mapping)"
    );
}

#[tokio::test]
async fn test_gas_price() {
    let (_, near) = super::support::shared_client().await;

    let gas_price = near.near().rpc().gas_price(None).await.unwrap();
    assert!(
        gas_price.gas_price > NearToken::from_yoctonear(0),
        "Gas price should be positive"
    );
}
