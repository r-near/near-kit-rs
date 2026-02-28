//! Integration tests verifying typed error deserialization from sandbox.
//!
//! These tests trigger real transaction failures and verify that
//! `ExecutionStatus::Failure(TxExecutionError)` deserializes correctly,
//! going through the RPC client directly to inspect raw outcomes.
//!
//! Run with: `cargo test --features sandbox --test integration typed_error`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("typerr{n}.{ROOT_ACCOUNT}").parse().unwrap()
}

/// Create a funded account, returning a Near client, the account ID, and the secret key.
async fn funded_account(
    sandbox: &near_kit::sandbox::SharedSandbox,
    balance: NearToken,
) -> (Near, AccountId, SecretKey) {
    let near = sandbox.client();
    let key = SecretKey::generate_ed25519();
    let id = unique_account();

    near.transaction(&id)
        .create_account()
        .transfer(balance)
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let client = Near::custom(sandbox.rpc_url())
        .credentials(key.to_string(), id.as_str())
        .unwrap()
        .build();

    (client, id, key)
}

/// Build, sign, and send a transaction via `rpc().send_tx()` so we get the
/// raw `FinalExecutionOutcome` without the client converting failures to `Err`.
async fn send_raw_tx(
    near: &Near,
    sender: &AccountId,
    key: &SecretKey,
    receiver: &AccountId,
    actions: Vec<Action>,
) -> FinalExecutionOutcome {
    let rpc = near.rpc();

    let ak = rpc
        .view_access_key(sender, &key.public_key(), BlockReference::final_())
        .await
        .unwrap();

    let block = rpc.block(BlockReference::final_()).await.unwrap();

    let tx = Transaction::new(
        sender.clone(),
        key.public_key(),
        ak.nonce + 1,
        receiver.clone(),
        block.header.hash,
        actions,
    );

    rpc.send_tx(&tx.sign(key), TxExecutionStatus::Final)
        .await
        .unwrap()
        .outcome
        .expect("expected execution outcome")
}

// =============================================================================
// ActionError deserialization
// =============================================================================

#[tokio::test]
async fn test_delete_nonexistent_key_deserializes_as_typed_error() {
    let sandbox = SandboxConfig::shared().await;
    let (near, id, key) = funded_account(sandbox, NearToken::near(10)).await;

    let fake_key = SecretKey::generate_ed25519();
    let outcome = send_raw_tx(
        &near,
        &id,
        &key,
        &id,
        vec![Action::DeleteKey(DeleteKeyAction {
            public_key: fake_key.public_key(),
        })],
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    match err {
        TxExecutionError::ActionError(ae) => match &ae.kind {
            ActionErrorKind::DeleteKeyDoesNotExist {
                account_id,
                public_key,
            } => {
                assert_eq!(account_id, &id);
                assert_eq!(public_key, &fake_key.public_key());
            }
            other => panic!("expected DeleteKeyDoesNotExist, got: {other:?}"),
        },
        other => panic!("expected ActionError, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_add_duplicate_key_deserializes_as_typed_error() {
    let sandbox = SandboxConfig::shared().await;
    let (near, id, key) = funded_account(sandbox, NearToken::near(10)).await;

    let outcome = send_raw_tx(
        &near,
        &id,
        &key,
        &id,
        vec![Action::AddKey(AddKeyAction {
            public_key: key.public_key(),
            access_key: AccessKey::full_access(),
        })],
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    match err {
        TxExecutionError::ActionError(ae) => match &ae.kind {
            ActionErrorKind::AddKeyAlreadyExists {
                account_id,
                public_key,
            } => {
                assert_eq!(account_id, &id);
                assert_eq!(public_key, &key.public_key());
            }
            other => panic!("expected AddKeyAlreadyExists, got: {other:?}"),
        },
        other => panic!("expected ActionError, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_stake_insufficient_balance_deserializes_as_typed_error() {
    let sandbox = SandboxConfig::shared().await;
    let (near, id, key) = funded_account(sandbox, NearToken::near(1)).await;

    let outcome = send_raw_tx(
        &near,
        &id,
        &key,
        &id,
        vec![Action::Stake(StakeAction {
            stake: NearToken::near(1000),
            public_key: key.public_key(),
        })],
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    match err {
        TxExecutionError::ActionError(ae) => match &ae.kind {
            ActionErrorKind::TriesToStake {
                account_id,
                stake,
                balance,
                ..
            } => {
                assert_eq!(account_id, &id);
                assert_eq!(*stake, NearToken::near(1000));
                assert!(balance.as_yoctonear() > 0);
            }
            other => panic!("expected TriesToStake, got: {other:?}"),
        },
        other => panic!("expected ActionError, got: {other:?}"),
    }
}

// =============================================================================
// FunctionCallError deserialization
// =============================================================================

#[tokio::test]
async fn test_call_nonexistent_method_deserializes_as_typed_error() {
    let sandbox = SandboxConfig::shared().await;
    let (near, id, key) = funded_account(sandbox, NearToken::near(50)).await;

    // Deploy a contract first
    let wasm = std::fs::read("tests/contracts/guestbook.wasm").unwrap();
    near.deploy(&id, wasm).await.unwrap();

    // Call a method that doesn't exist
    let outcome = send_raw_tx(
        &near,
        &id,
        &key,
        &id,
        vec![Action::FunctionCall(FunctionCallAction {
            method_name: "nonexistent_method".to_string(),
            args: vec![],
            gas: Gas::tgas(30),
            deposit: NearToken::yocto(0),
        })],
    )
    .await;

    // Function call errors appear in receipt outcomes, not the top-level status
    let failed = outcome
        .receipts_outcome
        .iter()
        .find(|r| matches!(r.outcome.status, ExecutionStatus::Failure(_)));

    let receipt = failed.expect("should have a failed receipt");
    match &receipt.outcome.status {
        ExecutionStatus::Failure(TxExecutionError::ActionError(ae)) => match &ae.kind {
            ActionErrorKind::FunctionCallError(FunctionCallError::MethodResolveError(
                MethodResolveError::MethodNotFound,
            )) => {
                // Correct!
            }
            other => panic!("expected FunctionCallError(MethodNotFound), got: {other:?}"),
        },
        other => panic!("expected Failure(ActionError), got: {other:?}"),
    }
}

// =============================================================================
// Display output (not raw JSON)
// =============================================================================

#[tokio::test]
async fn test_failure_message_is_human_readable_not_json() {
    let sandbox = SandboxConfig::shared().await;
    let (near, id, key) = funded_account(sandbox, NearToken::near(10)).await;

    let fake_key = SecretKey::generate_ed25519();
    let outcome = send_raw_tx(
        &near,
        &id,
        &key,
        &id,
        vec![Action::DeleteKey(DeleteKeyAction {
            public_key: fake_key.public_key(),
        })],
    )
    .await;

    let msg = outcome.failure_message().unwrap();
    assert!(
        !msg.contains("Object {"),
        "failure_message() should not contain raw JSON. Got: {msg}"
    );
    assert!(
        !msg.contains("\"kind\""),
        "failure_message() should not contain JSON keys. Got: {msg}"
    );
}
