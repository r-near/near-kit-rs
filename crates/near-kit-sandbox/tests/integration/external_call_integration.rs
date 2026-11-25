//! External contract calls (NEP-667) against a nearcore 2.15 sandbox.
//!
//! The contract is `contracts/external_call.wat`: its `__external_call` logs
//! "checking", refuses a payload starting with `r`, and otherwise stores the
//! payload, sends 1 yoctoNEAR to itself, logs "accepted" and returns "ok".

use near_kit::protocol::{
    Action, ActionErrorKind, ActionsValidationError, ExternalCall, GlobalContractId,
    InvalidTxError, PublishMode, TxExecutionError, UniversalStateInit, UniversalStateInitV1,
};
use near_kit::rpc::{
    AccountState, AuthorizationContext, BlockReference, ExecutionStatus, FinalExecutionOutcome,
    Finality, ReceiptContent, RpcError, TxExecutionStatus,
};
use near_kit::transaction::{ExecutedOptimistic, Final, FunctionCall};
use near_kit::{AccountId, CryptoHash, Error, Gas, Near, NearToken, SigningNear};
use near_kit_sandbox::Sandbox;

use super::support::funded_account;

/// nearcore 2.15's protocol version, which brings external calls.
const EXTERNAL_CALLS_PROTOCOL_VERSION: u32 = 89;

fn external_call_wasm() -> Vec<u8> {
    include_bytes!("../contracts/external_call.wasm").to_vec()
}

/// A fresh 2.15.0-rc.1 sandbox, checked to run protocol 89 (a tagged
/// `NEAR_SANDBOX_IMAGE` overrides the pinned version).
async fn sandbox_2_15() -> Sandbox {
    let sandbox = Sandbox::builder()
        .version("2.15.0-rc.1")
        .fresh()
        .await
        .expect("2.15 sandbox must start");
    let status = sandbox
        .root_account()
        .near()
        .rpc()
        .status()
        .await
        .expect("status");
    assert_eq!(
        status.protocol_version, EXTERNAL_CALLS_PROTOCOL_VERSION,
        "external calls need nearcore 2.15 (node {})",
        status.version.version
    );
    sandbox
}

/// An account with the external call contract deployed.
async fn contract(sandbox: &Sandbox, root: &SigningNear) -> AccountId {
    let (contract, contract_id, _) =
        funded_account(root, sandbox, "ecc", NearToken::from_near(10)).await;
    contract
        .wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(Action::deploy_contract(external_call_wasm()))
        .await
        .expect("deploy the external call contract")
        .result()
        .expect("deploy succeeds");
    contract_id
}

async fn height_of(near: &Near, block_hash: CryptoHash) -> u64 {
    near.rpc()
        .block(BlockReference::from(block_hash))
        .await
        .expect("block of the copy")
        .header
        .height
}

/// The copy's id is its unique id at the height of its block, and its
/// transaction view is the call.
async fn assert_copy_of(near: &Near, call: &ExternalCall, outcome: &FinalExecutionOutcome) -> u64 {
    let height = height_of(near, outcome.transaction_outcome.block_hash).await;
    assert_eq!(outcome.transaction_hash(), &call.unique_id(height));
    let view = outcome
        .transaction
        .as_external_call()
        .expect("an external call's transaction view");
    assert_eq!(&view.contract_id, call.contract_id());
    assert_eq!(view.payload, call.payload());
    assert_eq!(view.transaction_id, call.unique_id(height));
    assert_eq!(
        view.state_init.as_deref(),
        call.state_init().map(|s| s.0.as_slice())
    );
    assert_eq!(outcome.transaction.sender_id(), call.contract_id());
    assert_eq!(
        outcome.transaction_outcome.outcome.executor_id,
        *call.contract_id()
    );
    height
}

async fn stored_payload(near: &Near, contract_id: &AccountId) -> Option<Vec<u8>> {
    near.rpc()
        .view_state(contract_id, b"last", None, None, Finality::Final)
        .await
        .expect("view the contract state")
        .values
        .into_iter()
        .find(|item| item.key == b"last")
        .map(|item| item.value)
}

#[tokio::test]
async fn an_accepted_call_runs_and_is_found_by_its_unique_id() {
    let sandbox = sandbox_2_15().await;
    let root = sandbox.root_account();
    let near = root.near();
    let contract_id = contract(&sandbox, &root).await;
    let call = ExternalCall::new(contract_id.clone(), b"ok-1".to_vec());

    let dry_run = near
        .view_external_call(&call)
        .await
        .expect("the contract accepts the call");
    assert_eq!(dry_run.json::<String>().unwrap(), "ok");
    assert_eq!(dry_run.logs, ["checking", "accepted"]);
    assert_eq!(
        stored_payload(near, &contract_id).await,
        None,
        "a dry run writes nothing"
    );

    let before = near.balance(&contract_id).await.expect("balance").total;
    let outcome = near
        .send_external_call(&call)
        .await
        .expect("an accepted call");
    assert!(outcome.is_success(), "{outcome:?}");
    assert_eq!(outcome.json::<String>().unwrap(), "ok");
    assert_eq!(
        outcome.transaction_outcome.outcome.logs,
        ["checking", "accepted"]
    );
    let height = assert_copy_of(near, &call, &outcome).await;
    assert_eq!(
        stored_payload(near, &contract_id).await.as_deref(),
        Some(&b"ok-1"[..])
    );
    let after = near.balance(&contract_id).await.expect("balance").total;
    assert!(after < before, "the contract pays for its call");

    // `tx` takes the unique id, with the contract as the sender; the
    // receipts the call started carry its authorization context.
    let status = near
        .tx_status(&call.unique_id(height), contract_id.clone())
        .wait_until::<Final>()
        .await
        .expect("look the copy up by its unique id");
    assert_eq!(status.transaction_hash(), &call.unique_id(height));
    assert!(!status.receipts.is_empty());
    for receipt in &status.receipts {
        let ReceiptContent::Action(action) = &receipt.receipt else {
            panic!("{receipt:?}");
        };
        assert_eq!(action.signer_public_key, None, "no key signed the call");
        assert_eq!(
            action.authorization_context,
            Some(AuthorizationContext::ExternalContractCall)
        );
        assert_eq!(
            action.creation_protocol_version,
            Some(EXTERNAL_CALLS_PROTOCOL_VERSION)
        );
        assert_eq!(action.signer_id, contract_id);
    }
    let err = near
        .rpc()
        .tx_status(&call.call_hash(), &contract_id, TxExecutionStatus::None)
        .await
        .expect_err("the call hash is not an id");
    assert!(matches!(err, RpcError::TransactionNotFound(_)), "{err:?}");

    // A search finds the copy, and only at its height.
    let found = near
        .find_external_call(&call, height - 2..=height + 2)
        .await
        .expect("search")
        .expect("the copy");
    assert_eq!(found.transaction_hash(), &call.unique_id(height));
    assert!(
        near.find_external_call(&call, height + 1..=height + 2)
            .await
            .expect("search")
            .is_none()
    );

    // A node answers a repeat of a final, accepted call with its copy.
    let repeat = near
        .rpc()
        .send_external_call(&call, TxExecutionStatus::ExecutedOptimistic)
        .await
        .expect("repeat");
    assert_eq!(repeat.transaction_id(), Some(call.unique_id(height)));

    // Before inclusion there is no outcome; from inclusion on there is.
    let none = near
        .rpc()
        .send_external_call(
            &ExternalCall::new(contract_id.clone(), b"ok-2".to_vec()),
            TxExecutionStatus::None,
        )
        .await
        .expect("send at NONE");
    assert_eq!(none.final_execution_status, TxExecutionStatus::None);
    assert!(none.outcome.is_none());
    let included = near
        .rpc()
        .send_external_call(
            &ExternalCall::new(contract_id.clone(), b"ok-3".to_vec()),
            TxExecutionStatus::Included,
        )
        .await
        .expect("send at INCLUDED");
    let copy = included.outcome.expect("an included copy has its outcome");
    assert!(matches!(
        copy.transaction_outcome.outcome.status,
        ExecutionStatus::SuccessValue(_)
    ));

    // The 2.15 protocol config has the external call limits.
    let config: serde_json::Value = near
        .rpc()
        .request(
            "EXPERIMENTAL_protocol_config",
            serde_json::json!({ "finality": "final" }),
        )
        .await
        .expect("protocol config");
    let runtime = &config["runtime_config"];
    assert!(
        runtime["wasm_config"]["limit_config"]["max_gas_burnt_external"].as_u64() > Some(0),
        "{}",
        runtime["wasm_config"]["limit_config"]
    );
    assert_eq!(runtime["external_calls_config"]["max_calls_per_chunk"], 100);
    assert_eq!(runtime["external_calls_config"]["max_call_size"], 20480);
}

#[tokio::test]
async fn refusals_are_typed() {
    let sandbox = sandbox_2_15().await;
    let root = sandbox.root_account();
    let near = root.near();
    let contract_id = contract(&sandbox, &root).await;

    let refused = ExternalCall::new(contract_id.clone(), b"r-1".to_vec());
    let err = near
        .view_external_call(&refused)
        .at(Finality::Optimistic)
        .await
        .expect_err("the contract refuses the call");
    let err = err.as_rpc().expect("an RPC error");
    assert!(!err.is_retryable());
    match &err {
        RpcError::ExternalCallRefused {
            contract_id: refusing,
            error: Some(TxExecutionError::ActionError(action)),
            logs,
            block_height: Some(_),
            ..
        } => {
            assert_eq!(refusing, &contract_id);
            assert!(
                matches!(action.kind, ActionErrorKind::FunctionCallError(_)),
                "{action:?}"
            );
            assert_eq!(logs, &["checking"]);
        }
        other => panic!("{other:?}"),
    }

    // A view call of the reserved method is the same dry run.
    let err = near
        .view::<String>(contract_id.clone(), "__external_call")
        .args_raw(b"r-1".to_vec())
        .await
        .expect_err("the contract refuses the call");
    assert!(
        matches!(
            err.to_string().as_str(),
            message if message.contains(&format!("{contract_id} refused the external call"))
        ),
        "{err}"
    );

    // A state init that does not derive to the contract is refused too.
    let mismatched = ExternalCall::new(contract_id.clone(), b"ok".to_vec())
        .with_state_init(UniversalStateInitV1::default());
    let err = near
        .view_external_call(&mismatched)
        .await
        .expect_err("a state init for another account");
    assert!(
        matches!(
            err.as_rpc(),
            Some(RpcError::ExternalCallRefused {
                error: Some(TxExecutionError::InvalidTxError(
                    InvalidTxError::ActionsValidation(
                        ActionsValidationError::InvalidUniversalStateInitReceiver { .. }
                    )
                )),
                ..
            })
        ),
        "{err:?}"
    );

    // No contract: refused at the send.
    let root_call = ExternalCall::new(root.account_id().clone(), b"ok".to_vec());
    let err = near
        .send_external_call(&root_call)
        .search_until(u64::MAX)
        .await
        .expect_err("no contract");
    assert!(
        matches!(&err, Error::InvalidTx(e) if matches!(**e, InvalidTxError::ContractNotFound { .. })),
        "{err:?}"
    );
    let err = near
        .view_external_call(&root_call)
        .await
        .expect_err("no contract");
    assert!(
        matches!(
            err.as_rpc(),
            Some(RpcError::ExternalCallRefused {
                error: Some(TxExecutionError::InvalidTxError(
                    InvalidTxError::ContractNotFound { .. }
                )),
                ..
            })
        ),
        "{err:?}"
    );

    // Only an external call runs the reserved method.
    let outcome = root
        .wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(FunctionCall::new("__external_call").gas(Gas::from_tgas(10)))
        .await
        .expect("the transaction executes");
    assert!(
        matches!(
            outcome.failure_error(),
            Some(TxExecutionError::ActionError(e))
                if matches!(&e.kind, ActionErrorKind::ReservedMethodName { method_name, .. } if method_name == "__external_call")
        ),
        "{outcome:?}"
    );

    // Producers skip a refused call, so the send times out and the search
    // finds no copy. It starts just above the final head.
    let final_head = near
        .rpc()
        .block(Finality::Final)
        .await
        .expect("final block")
        .header
        .height;
    let head = near
        .rpc()
        .status()
        .await
        .expect("status")
        .sync_info
        .latest_block_height;
    let err = near
        .send_external_call(&refused)
        .wait_until::<ExecutedOptimistic>()
        .search_until(head + 5)
        .await
        .expect_err("a refused call is never included");
    match err {
        Error::ExternalCallNotFound {
            contract_id: searched,
            call_hash,
            heights,
            ..
        } => {
            assert_eq!(searched, contract_id);
            assert_eq!(call_hash, refused.call_hash());
            assert!(
                *heights.start() > final_head && *heights.end() == head + 5,
                "{heights:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(stored_payload(near, &contract_id).await, None);
}

#[tokio::test]
async fn a_call_with_a_state_init_initializes_a_universal_account() {
    let sandbox = sandbox_2_15().await;
    let root = sandbox.root_account();
    let near = root.near();

    let code = external_call_wasm();
    let code_hash = CryptoHash::hash(&code);
    root.wait_until::<Final>()
        .transaction(root.account_id())
        .add_action(Action::publish(code, PublishMode::Immutable))
        .await
        .expect("publish the contract")
        .result()
        .expect("publish succeeds");
    let state_init: UniversalStateInit = UniversalStateInitV1::default()
        .with_code(GlobalContractId::from(code_hash))
        .into();
    let account_id = state_init.derive_account_id();
    root.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::transfer(NearToken::from_near(5)))
        .await
        .expect("fund the 0u account")
        .result()
        .expect("transfer succeeds");

    let call =
        ExternalCall::new(account_id.clone(), b"ok-init".to_vec()).with_state_init(&state_init);
    // The global contract reaches the shard in a later block than its
    // publish; wait until a dry run with the state init can load it.
    let mut dry_run = None;
    for _ in 0..50 {
        match near.view_external_call(&call).await {
            Ok(result) => {
                dry_run = Some(result);
                break;
            }
            Err(err) if matches!(err.as_rpc(), Some(RpcError::ExternalCallRefused { .. })) => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(other) => panic!("{other:?}"),
        }
    }
    let dry_run = dry_run.expect("the dry run with the state init accepts the call");
    assert_eq!(dry_run.logs, ["checking", "accepted"]);
    let view = near
        .rpc()
        .view_account(&account_id, Finality::Final)
        .await
        .expect("view the 0u account");
    assert_eq!(
        view.state,
        AccountState::Uninitialized,
        "a dry run installs nothing"
    );

    let outcome = near
        .send_external_call(&call)
        .await
        .expect("an accepted call with a state init");
    assert!(outcome.is_success(), "{outcome:?}");
    let height = assert_copy_of(near, &call, &outcome).await;
    // The state init is not part of the id.
    assert_eq!(
        ExternalCall::new(account_id.clone(), b"ok-init".to_vec()).unique_id(height),
        call.unique_id(height)
    );

    let view = near
        .rpc()
        .view_account(&account_id, Finality::Final)
        .await
        .expect("view the 0u account");
    assert_eq!(view.state, AccountState::Initialized);
    assert_eq!(
        stored_payload(near, &account_id).await.as_deref(),
        Some(&b"ok-init"[..])
    );

    // Once the account is initialized, a call's state init is ignored.
    let again =
        ExternalCall::new(account_id.clone(), b"ok-again".to_vec()).with_state_init(&state_init);
    near.view_external_call(&again)
        .await
        .expect("an initialized account accepts a call with a state init");
    let outcome = near
        .send_external_call(&again)
        .await
        .expect("a second call with the state init");
    assert!(outcome.is_success(), "{outcome:?}");
    // The copy is included without the state init it no longer needs.
    let stripped = ExternalCall::new(account_id.clone(), b"ok-again".to_vec());
    assert_copy_of(near, &stripped, &outcome).await;
    assert_eq!(
        stored_payload(near, &account_id).await.as_deref(),
        Some(&b"ok-again"[..])
    );
}

/// A call whose encoding is `encoded_len` bytes long, which the contract
/// accepts.
fn call_of_size(contract_id: &AccountId, encoded_len: usize) -> ExternalCall {
    // 0xFF, the version, the contract and payload lengths, no state init.
    let overhead = 2 + 4 + contract_id.len() + 4 + 1;
    let call = ExternalCall::new(contract_id.clone(), vec![b'o'; encoded_len - overhead]);
    assert_eq!(call.to_bytes().len(), encoded_len);
    call
}

#[tokio::test]
async fn the_size_limit_counts_the_whole_encoding() {
    let sandbox = sandbox_2_15().await;
    let root = sandbox.root_account();
    let near = root.near();
    let contract_id = contract(&sandbox, &root).await;

    let largest = call_of_size(&contract_id, 20480);
    near.view_external_call(&largest)
        .await
        .expect("a call at the limit");
    let outcome = near
        .send_external_call(&largest)
        .wait_until::<ExecutedOptimistic>()
        .await
        .expect("a call at the limit");
    assert!(outcome.is_success(), "{outcome:?}");
    assert_copy_of(near, &largest, &outcome).await;

    let too_large = call_of_size(&contract_id, 20481);
    let err = near
        .view_external_call(&too_large)
        .await
        .expect_err("a call over the limit");
    assert!(
        matches!(
            err.as_rpc(),
            Some(RpcError::ExternalCallRefused {
                error: Some(TxExecutionError::InvalidTxError(
                    InvalidTxError::TransactionSizeExceeded {
                        size: 20481,
                        limit: 20480,
                        ..
                    }
                )),
                ..
            })
        ),
        "{err:?}"
    );
    let err = near
        .send_external_call(&too_large)
        .search_until(u64::MAX)
        .await
        .expect_err("a call over the limit");
    assert!(
        matches!(
            &err,
            Error::InvalidTx(e)
                if matches!(**e, InvalidTxError::TransactionSizeExceeded { size: 20481, limit: 20480, .. })
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn concurrent_sends_of_a_call_share_one_copy() {
    let sandbox = sandbox_2_15().await;
    let root = sandbox.root_account();
    let near = root.near();
    let contract_id = contract(&sandbox, &root).await;

    let call = ExternalCall::new(contract_id.clone(), b"ok-twice".to_vec());
    let (first, second) = tokio::join!(
        near.send_external_call(&call),
        near.send_external_call(&call)
    );
    let first = first.expect("the first send");
    let second = second.expect("the second send");
    assert_eq!(first.transaction_hash(), second.transaction_hash());
    assert_copy_of(near, &call, &first).await;
}

#[tokio::test]
async fn a_node_before_2_15_cannot_parse_an_external_call() {
    let sandbox = Sandbox::builder()
        .version("2.14.0")
        .fresh()
        .await
        .expect("2.14 sandbox must start");
    let root = sandbox.root_account();
    let near = root.near();
    let status = near.rpc().status().await.expect("status");
    if status.protocol_version >= EXTERNAL_CALLS_PROTOCOL_VERSION
        && std::env::var_os("NEAR_SANDBOX_IMAGE").is_some()
    {
        eprintln!(
            "skipped: NEAR_SANDBOX_IMAGE replaced the 2.14.0 pin with nearcore {}",
            status.version.version
        );
        return;
    }
    assert!(
        status.protocol_version < EXTERNAL_CALLS_PROTOCOL_VERSION,
        "this test needs a node before 2.15 (node {})",
        status.version.version
    );
    let contract_id = contract(&sandbox, &root).await;
    let call = ExternalCall::new(contract_id, b"ok".to_vec());
    let err = near
        .send_external_call(&call)
        .search_until(u64::MAX)
        .await
        .expect_err("2.14 has no external calls");
    assert!(
        matches!(err.as_rpc(), Some(RpcError::ParseError(_))),
        "{err:?}"
    );
}
