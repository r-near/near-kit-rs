//! Integration tests verifying the unified error handling.
//!
//! These tests ensure that:
//! - `InvalidTxError` surfaces as `Err(Error::InvalidTx(..))` regardless of
//!   whether the RPC or runtime caught it
//! - Action errors (contract panics, missing keys, etc.) return `Ok(outcome)`
//!   where `outcome.is_failure()` is `true`
//! - Successful transactions return `Ok(outcome)` where `outcome.is_success()`
//!
//! Run with: `cargo test --features sandbox --test integration error_consolidation`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("errcon{n}.{SANDBOX_ROOT_ACCOUNT}").parse().unwrap()
}

// =============================================================================
// Successful transactions return Ok with is_success()
// =============================================================================

#[tokio::test]
async fn test_successful_transfer_returns_ok() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account — should succeed
    let outcome = near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(5))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect("Successful transaction should return Ok");

    assert!(outcome.is_success());
    assert!(!outcome.transaction_hash().is_zero());
}

// =============================================================================
// Action errors return Ok(outcome) with is_failure() true
// =============================================================================

#[tokio::test]
async fn test_action_error_returns_ok_with_failure_outcome() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), &account_id)
        .unwrap()
        .build();

    // Delete a key that doesn't exist — ActionError, but returned as Ok
    let fake_key = SecretKey::generate_ed25519();
    let outcome = account_near
        .transaction(&account_id)
        .delete_key(fake_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect("Action errors should return Ok(outcome)");

    assert!(outcome.is_failure(), "Outcome should be a failure");
    assert!(!outcome.is_success());
    assert!(
        outcome.failure_message().is_some(),
        "Should have a failure message"
    );
    assert!(
        outcome.total_gas_used().as_gas() > 0,
        "Gas should have been consumed"
    );
    assert!(!outcome.transaction_hash().is_zero());
}

#[tokio::test]
async fn test_function_call_error_returns_ok_with_failure_outcome() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let contract_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), &contract_id)
        .unwrap()
        .build();

    // Call a non-existent method — returns Ok with failure outcome
    let outcome = contract_near
        .call(&contract_id, "nonexistent_method")
        .args(serde_json::json!({}))
        .gas(Gas::from_tgas(30))
        .await
        .expect("Action errors should return Ok(outcome)");

    assert!(outcome.is_failure());
    assert!(outcome.failure_message().is_some());
    // Receipts should exist (the tx was executed)
    assert!(!outcome.receipts_outcome.is_empty());
}

// =============================================================================
// InvalidTxError surfaces as Err(Error::InvalidTx) from the RPC path
// =============================================================================

#[tokio::test]
#[ignore = "requires EXPERIMENTAL_view_access_key (nearcore 2.11+), see #105"]
async fn test_wrong_signer_key_returns_invalid_tx_or_rpc_error() {
    let sandbox = SandboxConfig::shared().await;

    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account with one key
    let near = sandbox.client();
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(5))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to sign with a wrong key
    let wrong_key = SecretKey::generate_ed25519();
    let wrong_near = Near::custom(sandbox.rpc_url())
        .credentials(wrong_key.to_string(), &account_id)
        .unwrap()
        .build();

    let err = wrong_near
        .transfer(&account_id, NearToken::from_near(1))
        .await
        .expect_err("Wrong key should fail");

    // The wrong key is caught during nonce lookup (view_access_key) before
    // the transaction is even built.
    match &err {
        Error::Rpc(e) => match e.as_ref() {
            RpcError::AccessKeyNotFound {
                account_id: err_account,
                ..
            } => {
                assert_eq!(err_account, &account_id);
            }
            other => panic!("Expected AccessKeyNotFound, got: {other:?}"),
        },
        other => panic!("Expected Rpc(AccessKeyNotFound), got: {other:?}"),
    }

    println!("Wrong key error: {:?}", err);
}

// =============================================================================
// RpcError::InvalidTx promotes to Error::InvalidTx via From
// =============================================================================

#[test]
fn test_rpc_invalid_tx_promotes_to_error_invalid_tx() {
    let rpc_err = RpcError::InvalidTx(InvalidTxError::InvalidNonce {
        tx_nonce: 5,
        ak_nonce: 10,
    });
    let err: Error = rpc_err.into();
    match err {
        Error::InvalidTx(e) => match *e {
            InvalidTxError::InvalidNonce { tx_nonce, ak_nonce } => {
                assert_eq!(tx_nonce, 5);
                assert_eq!(ak_nonce, 10);
            }
            other => panic!("Expected InvalidNonce, got: {other:?}"),
        },
        other => panic!("Expected Error::InvalidTx(InvalidNonce), got: {other:?}"),
    }
}

#[test]
fn test_rpc_non_tx_error_stays_as_rpc() {
    let rpc_err = RpcError::Timeout(3);
    let err: Error = rpc_err.into();
    assert!(matches!(err, Error::Rpc(ref e) if matches!(e.as_ref(), RpcError::Timeout(3))));
}

// =============================================================================
// InvalidTxError::is_retryable
// =============================================================================

#[test]
fn test_invalid_tx_error_retryable() {
    assert!(
        InvalidTxError::InvalidNonce {
            tx_nonce: 1,
            ak_nonce: 2
        }
        .is_retryable()
    );
    assert!(
        InvalidTxError::ShardCongested {
            congestion_level: 0.9,
            shard_id: 0
        }
        .is_retryable()
    );
    assert!(
        InvalidTxError::ShardStuck {
            missed_chunks: 5,
            shard_id: 0
        }
        .is_retryable()
    );
    assert!(!InvalidTxError::Expired.is_retryable());
    assert!(!InvalidTxError::InvalidSignature.is_retryable());
}
