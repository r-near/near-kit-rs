//! Integration tests for FinalExecutionOutcome receipt inspection on failure.
//!
//! Verifies that failed transactions still expose the full outcome,
//! allowing callers to inspect receipts, logs, and gas usage.
//!
//! Run with: `cargo test --features sandbox --test integration -- transaction_outcome`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("txout{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

#[tokio::test]
async fn test_failed_transaction_preserves_receipts() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy the guestbook contract
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
        .expect("setup: deploy should succeed")
        .result()
        .expect("setup: deploy should succeed on-chain");

    let account_near = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), &contract_id)
        .unwrap()
        .build();

    // Call a non-existent method — the transaction executes but fails on-chain
    let err = account_near
        .call(&contract_id, "nonexistent_method")
        .args(serde_json::json!({}))
        .gas(Gas::from_tgas(10))
        .await
        .expect_err("Should fail for non-existent method");

    // Should be an ActionFailed error with the outcome attached
    match &err {
        Error::ActionFailed { error, outcome } => {
            println!("Got expected action error: {error}");

            // Receipt details are accessible via the attached outcome
            assert!(
                !outcome.receipts_outcome.is_empty(),
                "Should have receipt outcomes even on failure"
            );
            assert!(outcome.total_gas_used().as_gas() > 0);
            assert!(!outcome.transaction_hash().is_zero());

            for (i, receipt) in outcome.receipts_outcome.iter().enumerate() {
                println!(
                    "Receipt {i}: executor={}, status={:?}",
                    receipt.outcome.executor_id, receipt.outcome.status
                );
            }
        }
        other => panic!("Expected ActionFailed, got: {other:?}"),
    }
}
