//! Integration tests verifying typed error deserialization from sandbox.
//!
//! These tests trigger real transaction failures and verify that
//! `ExecutionStatus::Failure(ActionError)` deserializes correctly in the
//! outcome a send returns.

use near_kit::protocol::Action;
use near_kit::transaction::Final;
use near_kit::*;
use near_kit::{protocol::*, rpc::*, signer::*};

use super::support::{funded_account, guestbook_wasm, shared_client};

/// Send `action` from `near`'s account to itself and return the outcome,
/// which a failing action leaves as `Ok`.
async fn send(near: &SigningNear, action: Action) -> FinalExecutionOutcome {
    near.wait_until::<Final>()
        .transaction(near.account_id())
        .add_action(action)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_delete_nonexistent_key_deserializes_as_typed_error() {
    let (sandbox, root) = shared_client().await;
    let (near, id, _) = funded_account(&root, sandbox, "typerr", NearToken::from_near(10)).await;

    let fake_key = SecretKey::generate_ed25519();
    let outcome = send(
        &near,
        Action::DeleteKey(DeleteKeyAction {
            public_key: fake_key.public_key(),
        }),
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    let TxExecutionError::ActionError(ae) = err else {
        panic!("expected ActionError, got: {err:?}");
    };
    assert!(
        matches!(
            &ae.kind,
            ActionErrorKind::DeleteKeyDoesNotExist { account_id, public_key }
                if account_id == &id && public_key == &fake_key.public_key()
        ),
        "expected DeleteKeyDoesNotExist, got: {:?}",
        ae.kind
    );

    // The failure message is human-readable, not raw JSON.
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

#[tokio::test]
async fn test_add_duplicate_key_deserializes_as_typed_error() {
    let (sandbox, root) = shared_client().await;
    let (near, id, key) = funded_account(&root, sandbox, "typerr", NearToken::from_near(10)).await;

    let outcome = send(
        &near,
        Action::AddKey(AddKeyAction {
            public_key: key.public_key(),
            access_key: AccessKey::full_access(),
        }),
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    let TxExecutionError::ActionError(ae) = err else {
        panic!("expected ActionError, got: {err:?}");
    };
    assert!(
        matches!(
            &ae.kind,
            ActionErrorKind::AddKeyAlreadyExists { account_id, public_key }
                if account_id == &id && public_key == &key.public_key()
        ),
        "expected AddKeyAlreadyExists, got: {:?}",
        ae.kind
    );
}

#[tokio::test]
async fn test_stake_insufficient_balance_deserializes_as_typed_error() {
    let (sandbox, root) = shared_client().await;
    let (near, id, key) = funded_account(&root, sandbox, "typerr", NearToken::from_near(1)).await;

    let outcome = send(
        &near,
        Action::Stake(StakeAction {
            stake: NearToken::from_near(1000),
            public_key: key.public_key(),
        }),
    )
    .await;

    assert!(outcome.is_failure());

    let err = outcome.failure_error().expect("should have typed error");
    let TxExecutionError::ActionError(ae) = err else {
        panic!("expected ActionError, got: {err:?}");
    };
    assert!(
        matches!(
            &ae.kind,
            ActionErrorKind::TriesToStake { account_id, stake, balance, .. }
                if account_id == &id
                    && *stake == NearToken::from_near(1000)
                    && balance.as_yoctonear() > 0
        ),
        "expected TriesToStake, got: {:?}",
        ae.kind
    );
}

#[tokio::test]
async fn test_call_nonexistent_method_deserializes_as_typed_error() {
    let (sandbox, root) = shared_client().await;
    let (near, _, _) = funded_account(&root, sandbox, "typerr", NearToken::from_near(50)).await;

    near.transaction(near.account_id())
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .unwrap()
        .result()
        .unwrap();

    let outcome = send(
        &near,
        Action::FunctionCall(FunctionCallAction {
            method_name: "nonexistent_method".to_string(),
            args: vec![],
            gas: Gas::from_tgas(30),
            deposit: NearToken::from_yoctonear(0),
        }),
    )
    .await;

    // Function call errors appear in receipt outcomes, not the top-level status
    let failed = outcome
        .receipts_outcome
        .iter()
        .find(|r| matches!(r.outcome.status, ExecutionStatus::Failure(_)))
        .expect("should have a failed receipt");
    let ExecutionStatus::Failure(ae) = &failed.outcome.status else {
        unreachable!("found by its failure status");
    };
    assert!(
        matches!(
            &ae.kind,
            ActionErrorKind::FunctionCallError(FunctionCallError::MethodResolveError(
                MethodResolveError::MethodNotFound,
            ))
        ),
        "expected FunctionCallError(MethodNotFound), got: {:?}",
        ae.kind
    );
}
