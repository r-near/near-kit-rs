//! Integration tests verifying the unified error handling.
//!
//! These tests ensure that:
//! - `InvalidTxError` surfaces as `Err(Error::InvalidTx(..))` regardless of
//!   whether the RPC or runtime caught it
//! - `ActionError` surfaces as `Err(Error::ActionFailed { error, outcome })`
//!   with the full outcome attached
//! - Successful transactions return `Ok(outcome)`
//!
//! Run with: `cargo test --features sandbox --test integration error_consolidation`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("errcon{n}.{ROOT_ACCOUNT}").parse().unwrap()
}

// =============================================================================
// Successful transactions return Ok
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
// ActionError returns Err(Error::ActionFailed) with outcome attached
// =============================================================================

#[tokio::test]
async fn test_action_error_returns_err_with_outcome() {
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

    // Delete a key that doesn't exist — ActionError
    let fake_key = SecretKey::generate_ed25519();
    let err = account_near
        .transaction(&account_id)
        .delete_key(fake_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .expect_err("Deleting non-existent key should fail");

    // Verify it's ActionFailed
    match &err {
        Error::ActionFailed { error, outcome } => {
            // ActionError should name the specific kind
            match &error.kind {
                ActionErrorKind::DeleteKeyDoesNotExist {
                    account_id: err_account,
                    public_key,
                } => {
                    assert_eq!(err_account, &account_id);
                    assert_eq!(public_key, &fake_key.public_key());
                }
                other => panic!("Expected DeleteKeyDoesNotExist, got: {other:?}"),
            }

            // Outcome should be attached with meaningful data
            assert!(
                outcome.total_gas_used().as_gas() > 0,
                "Gas should have been consumed"
            );
            assert!(!outcome.transaction_hash().is_zero());
        }
        other => panic!("Expected ActionFailed, got: {other:?}"),
    }

    // Convenience methods should work
    assert!(err.is_action_failed());
    assert!(!err.is_invalid_tx());
    assert!(err.outcome().is_some());
}

#[tokio::test]
async fn test_function_call_error_returns_action_failed() {
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

    // Call a non-existent method
    let err = contract_near
        .call(&contract_id, "nonexistent_method")
        .args(serde_json::json!({}))
        .gas(Gas::from_tgas(30))
        .await
        .expect_err("Non-existent method should fail");

    match &err {
        Error::ActionFailed { error, outcome } => {
            // Should be a FunctionCallError
            match &error.kind {
                ActionErrorKind::FunctionCallError(_) => { /* expected */ }
                other => panic!("Expected FunctionCallError, got: {other:?}"),
            }
            // Receipts should exist (the tx was executed)
            assert!(!outcome.receipts_outcome.is_empty());
        }
        other => panic!("Expected ActionFailed, got: {other:?}"),
    }
}

// =============================================================================
// InvalidTxError surfaces as Err(Error::InvalidTx) from the RPC path
// =============================================================================

#[tokio::test]
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

    // Could be InvalidTx (if caught by runtime) or Rpc error (if caught at RPC layer)
    match &err {
        Error::InvalidTx(_) => { /* expected: runtime caught it */ }
        Error::Rpc(_) => { /* also ok: RPC pre-check caught it (AccessKeyNotFound, JSON parse error, etc.) */
        }
        other => panic!("Expected InvalidTx or Rpc error, got: {other:?}"),
    }

    println!("Wrong key error: {:?}", err);
}

// =============================================================================
// Error::outcome() gives None for non-ActionFailed errors
// =============================================================================

#[tokio::test]
async fn test_error_outcome_returns_none_for_non_action_errors() {
    let err = Error::InvalidTx(InvalidTxError::Expired);
    assert!(err.outcome().is_none());
    assert!(err.is_invalid_tx());
    assert!(!err.is_action_failed());
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
        Error::InvalidTx(InvalidTxError::InvalidNonce { tx_nonce, ak_nonce }) => {
            assert_eq!(tx_nonce, 5);
            assert_eq!(ak_nonce, 10);
        }
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
