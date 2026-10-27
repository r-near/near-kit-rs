//! Integration tests for global contracts and all action types.
//!
//! These tests exercise the global contract actions (DeployGlobalContract, UseGlobalContract)
//! and NEP-616 deterministic account creation, as well as all other action types
//! built from `Action::*` constructors.
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_kit::*;
use near_kit::{protocol::*, signer::*, transaction::Final};
use std::collections::BTreeMap;

use super::support::{funded_account, guestbook_wasm, unique_account};

/// Load the test contract WASM
fn load_test_contract() -> Vec<u8> {
    guestbook_wasm()
}

// =============================================================================
// Helper: Create a funded account
// =============================================================================

async fn create_funded_account(
    root_near: &SigningNear,
    sandbox: &near_kit_sandbox::Sandbox,
    funding: NearToken,
) -> (SigningNear, AccountId, SecretKey) {
    funded_account(root_near, sandbox, "gc", funding).await
}

/// Poll until a published global contract is visible at the final block.
///
/// `DeployGlobalContract` hands the code to each shard through a separate
/// `GlobalContractDistribution` receipt. That receipt has no execution outcome
/// and is applied in a later block than the deploy action, so
/// `wait_until::<Final>()` on the publish transaction does not cover it. A
/// query at `final` straight after the publish normally works only because
/// the gas-refund receipt (which the outcome does cover) happens to land in
/// the same block as the distribution; under load the distribution can land
/// later and the query sees `GlobalContractNotFound`. Bounded so a genuinely
/// failed publish still fails the test instead of hanging.
async fn wait_for_global_contract(near: &SigningNear, id: impl Into<GlobalContractId>) {
    const ATTEMPTS: u32 = 50;
    let id = id.into();
    for _ in 0..ATTEMPTS {
        if global_contract_exists(near.near(), &id).await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("global contract not visible at final after {ATTEMPTS} polls");
}

/// Whether anything is published under `id`: "not found" (and, for a
/// publisher account, "no such account") reads as `false`.
async fn global_contract_exists(near: &Near, id: &GlobalContractId) -> bool {
    match near
        .rpc()
        .view_global_contract_code(id, Default::default())
        .await
    {
        Ok(_) => true,
        Err(e) if e.is_global_contract_not_found() || e.is_account_not_found() => false,
        Err(e) => panic!("unexpected error: {e:?}"),
    }
}

// =============================================================================
// Global Contract Tests
// =============================================================================

/// End-to-end test: publish → deploy_from → call the deployed contract.
#[tokio::test]
async fn test_publish_deploy_from_call_end_to_end() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Publisher publishes an updatable contract
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    publisher_near
        .wait_until::<Final>()
        .transaction(publisher_near.account_id())
        .add_action(Action::publish(wasm_code, PublishMode::Updatable))
        .await
        .unwrap();

    // User deploys from the publisher's global contract
    let (user_near, user_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(10)).await;

    user_near
        .wait_until::<Final>()
        .transaction(user_near.account_id())
        .add_action(Action::deploy_from_account(publisher_id.clone()))
        .await
        .unwrap();

    // Call a mutating method on the deployed contract
    user_near
        .wait_until::<Final>()
        .transaction(&user_id)
        .add_action(
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "Hello from global contract!" }))
                .gas(Gas::from_tgas(30))
                .deposit(NearToken::ZERO),
        )
        .await
        .unwrap();

    // Verify the message was stored
    let messages: Vec<serde_json::Value> = root_near
        .near()
        .view(&user_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["text"], "Hello from global contract!");
}

/// Test publishing a contract to the global registry by account ID (updatable).
#[tokio::test]
async fn test_publish_contract_by_account() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    // Publish the contract in updatable mode (identified by the publisher account)
    let outcome = publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Updatable))
        .await
        .unwrap();

    println!(
        "Publish contract succeeded: {:?}",
        outcome.transaction_hash()
    );
}

/// Test publishing a contract to the global registry by code hash (immutable).
#[tokio::test]
async fn test_publish_contract_by_hash() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    // Publish the contract using PublishMode::Immutable (identified by code hash, immutable)
    let outcome = publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Immutable))
        .await
        .unwrap();

    println!(
        "Publish contract by hash succeeded: {:?}",
        outcome.transaction_hash()
    );
}

/// Test deploying a contract from a publisher account.
#[tokio::test]
async fn test_deploy_from_publisher() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    // First, publish the contract
    publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Updatable))
        .await
        .unwrap();

    // Create a user account that will deploy from the publisher
    let (user_near, user_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(10)).await;

    // Deploy from the publisher's global contract (passing AccountId directly)
    let outcome = user_near
        .wait_until::<Final>()
        .transaction(&user_id)
        .add_action(Action::deploy_from_account(publisher_id.clone()))
        .await
        .unwrap();

    println!(
        "Deploy from publisher succeeded: {:?}",
        outcome.transaction_hash()
    );

    // Verify the contract was deployed by calling a view method
    // (with global contracts, the account references global code, not a local code_hash)
    // The guestbook contract expects empty JSON object {} for get_messages
    let messages: Vec<serde_json::Value> = root_near
        .near()
        .view(&user_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .unwrap();
    assert!(messages.is_empty()); // New guestbook should have no messages
}

/// Test deploying a contract from a code hash.
#[tokio::test]
async fn test_deploy_from_hash() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    // Calculate the code hash before publishing
    let code_hash = CryptoHash::hash(&wasm_code);
    println!("Contract code hash: {}", code_hash);

    // Publish the contract by hash (immutable)
    publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Immutable))
        .await
        .unwrap();

    // Create a user account that will deploy from the hash
    let (user_near, user_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(10)).await;

    // Deploy from the code hash
    let outcome = user_near
        .wait_until::<Final>()
        .transaction(&user_id)
        .add_action(Action::deploy_from_hash(code_hash))
        .await
        .unwrap();

    println!(
        "Deploy from hash succeeded: {:?}",
        outcome.transaction_hash()
    );

    // Verify the contract was deployed by calling a view method
    // (with global contracts, the account references global code, not a local code_hash)
    // The guestbook contract expects empty JSON object {} for get_messages
    let messages: Vec<serde_json::Value> = root_near
        .near()
        .view(&user_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .unwrap();
    assert!(messages.is_empty()); // New guestbook should have no messages
}

/// Test NEP-616 deterministic state init with code hash.
#[tokio::test]
async fn test_state_init_by_hash() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();
    let code_hash = CryptoHash::hash(&wasm_code);

    // First, publish the contract by hash
    publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Immutable))
        .await
        .unwrap();

    // Create initial state data (empty for this test)
    let initial_data: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    // Use state_init to create a deterministic account
    let si = StateInit::by_hash(code_hash, initial_data);
    let receiver = si.derive_account_id();
    let outcome = publisher_near
        .wait_until::<Final>()
        .transaction(&receiver)
        .add_action(Action::state_init(si, NearToken::from_near(5)))
        .await
        .unwrap();

    println!(
        "State init by hash succeeded: {:?}",
        outcome.transaction_hash()
    );
}

/// Test NEP-616 deterministic state init with publisher account.
#[tokio::test]
async fn test_state_init_by_publisher() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    // First, publish the contract by account
    publisher_near
        .wait_until::<Final>()
        .transaction(&publisher_id)
        .add_action(Action::publish(wasm_code, PublishMode::Updatable))
        .await
        .unwrap();

    // Create initial state data with some sample data
    let mut initial_data: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    initial_data.insert(b"key1".to_vec(), b"value1".to_vec());

    // Use state_init to create a deterministic account
    let si = StateInit::by_publisher(publisher_id.clone(), initial_data);
    let receiver = si.derive_account_id();
    let outcome = publisher_near
        .wait_until::<Final>()
        .transaction(&receiver)
        .add_action(Action::state_init(si, NearToken::from_near(5)))
        .await
        .unwrap();

    println!(
        "State init by publisher succeeded: {:?}",
        outcome.transaction_hash()
    );
}

// =============================================================================
// Action Type Tests - Exercise the remaining `Action` constructors
// =============================================================================

/// Test AddKey action with function call access
#[tokio::test]
async fn test_action_add_function_call_key() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (account_near, account_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(10)).await;

    let fc_key = SecretKey::generate_ed25519();
    let receiver_contract: AccountId = near_kit::account!("some-contract.sandbox");

    let _outcome = account_near
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::add_function_call_key(
            fc_key.public_key(),
            receiver_contract,
            vec!["method1".to_string(), "method2".to_string()],
            Some(NearToken::from_near(1)),
        ))
        .await
        .unwrap();

    // Verify the key was added
    let keys = root_near
        .near()
        .rpc()
        .view_access_key_list(&account_id, Default::default())
        .await
        .unwrap();
    assert_eq!(keys.keys.len(), 2);
}

/// Test DeleteAccount action
#[tokio::test]
async fn test_action_delete_account() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (beneficiary_near, beneficiary_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(10)).await;

    // Create an account to delete
    let to_delete_key = SecretKey::generate_ed25519();
    let to_delete_id: AccountId = format!("todelete.{}", beneficiary_id).parse().unwrap();

    beneficiary_near
        .wait_until::<Final>()
        .transaction(&to_delete_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(2)))
        .add_action(Action::add_full_access_key(to_delete_key.public_key()))
        .await
        .unwrap();

    assert!(
        root_near
            .near()
            .account_exists(&to_delete_id)
            .await
            .unwrap()
    );

    // Delete the account
    let to_delete_near = sandbox.client().near().with_signer(InMemorySigner::new(
        &to_delete_id,
        to_delete_key.to_string().parse().unwrap(),
    ));

    to_delete_near
        .wait_until::<Final>()
        .transaction(&to_delete_id)
        .add_action(Action::delete_account(beneficiary_id.clone()))
        .await
        .unwrap();

    assert!(
        !root_near
            .near()
            .account_exists(&to_delete_id)
            .await
            .unwrap()
    );
}

/// Test Stake action
#[tokio::test]
async fn test_action_stake() {
    let (sandbox, root_near) = super::support::shared_client().await;

    // Create a validator account with small initial balance
    let (staker_near, staker_id, staker_key) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(100)).await;

    // Patch the balance to 2M NEAR (enough to meet sandbox minimum stake of ~800K)
    let staking_balance = NearToken::from_near(2_000_000);
    sandbox
        .set_balance(&staker_id, staking_balance)
        .await
        .unwrap();

    // Verify the patched balance
    let balance = root_near.near().balance(&staker_id).await.unwrap();
    assert_eq!(balance.total, staking_balance);

    // Now stake with enough to meet the minimum
    let stake_amount = NearToken::from_near(1_000_000);
    let outcome = staker_near
        .wait_until::<Final>()
        .transaction(&staker_id)
        .add_action(Action::stake(stake_amount, staker_key.public_key()))
        .await
        .unwrap();

    println!("Stake action completed: {:?}", outcome.transaction_hash());

    // Verify locked balance reflects the stake
    let account = root_near
        .near()
        .rpc()
        .view_account(&staker_id, Default::default())
        .await
        .unwrap();
    println!("Locked balance after staking: {}", account.locked);
    assert!(account.locked >= stake_amount);
}

/// Test chaining multiple function calls
#[tokio::test]
async fn test_multiple_function_calls() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (contract_near, contract_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(20)).await;

    let wasm_code = load_test_contract();

    // Deploy the contract
    contract_near
        .wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(Action::deploy_contract(wasm_code))
        .await
        .unwrap();

    // Call multiple methods in one transaction
    let outcome = contract_near
        .wait_until::<Final>()
        .transaction(&contract_id)
        .add_action(
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "First message" }))
                .gas(Gas::from_tgas(15)),
        )
        .add_action(
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "Second message" }))
                .gas(Gas::from_tgas(15)),
        )
        .await
        .unwrap();

    println!(
        "Multiple function calls: gas used = {}",
        outcome.total_gas_used()
    );
}

// =============================================================================
// Global Contract Query Tests (near.global_contract / near.contract_code)
// =============================================================================

/// Query a published (updatable) global contract by publisher account.
#[tokio::test]
async fn test_global_contract_query_by_account() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();

    publisher_near
        .wait_until::<Final>()
        .transaction(publisher_near.account_id())
        .add_action(Action::publish(wasm_code.clone(), PublishMode::Updatable))
        .await
        .unwrap();
    wait_for_global_contract(&root_near, publisher_id.clone()).await;

    let id = GlobalContractId::from(publisher_id.clone());
    let contract = root_near
        .near()
        .rpc()
        .view_global_contract_code(&id, Default::default())
        .await
        .unwrap();
    assert_eq!(contract.code, wasm_code);
    assert_eq!(contract.hash, CryptoHash::hash(&wasm_code));
    assert!(global_contract_exists(root_near.near(), &id).await);
}

/// Query a published (immutable) global contract by code hash.
#[tokio::test]
async fn test_global_contract_query_by_hash() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (publisher_near, _, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;

    let wasm_code = load_test_contract();
    let code_hash = CryptoHash::hash(&wasm_code);

    publisher_near
        .wait_until::<Final>()
        .transaction(publisher_near.account_id())
        .add_action(Action::publish(wasm_code.clone(), PublishMode::Immutable))
        .await
        .unwrap();
    wait_for_global_contract(&root_near, *code_hash.as_bytes()).await;

    let id = GlobalContractId::from(*code_hash.as_bytes());
    let contract = root_near
        .near()
        .rpc()
        .view_global_contract_code(&id, Default::default())
        .await
        .unwrap();
    assert_eq!(contract.code, wasm_code);
    assert_eq!(contract.hash, code_hash);
    assert!(global_contract_exists(root_near.near(), &id).await);
}

/// Missing global contracts surface a typed error, which reads as "not
/// published".
#[tokio::test]
async fn test_global_contract_query_not_found() {
    let (_, root_near) = super::support::shared_client().await;

    // An account that never published anything
    let unpublished = GlobalContractId::from(unique_account("gc"));
    let err = root_near
        .near()
        .rpc()
        .view_global_contract_code(&unpublished, Default::default())
        .await
        .unwrap_err();
    assert!(
        err.is_global_contract_not_found(),
        "unexpected error: {err:?}"
    );
    assert!(!global_contract_exists(root_near.near(), &unpublished).await);

    // A hash nothing was published under
    let bogus_hash = CryptoHash::hash(b"no contract with this hash");
    assert!(
        !global_contract_exists(
            root_near.near(),
            &GlobalContractId::from(*bogus_hash.as_bytes())
        )
        .await
    );
}

/// `RpcClient::view_code` returns the code deployed on a regular account,
/// and a typed error before anything is deployed.
#[tokio::test]
async fn test_contract_code_query() {
    let (sandbox, root_near) = super::support::shared_client().await;

    let (account_near, account_id, _) =
        create_funded_account(&root_near, sandbox, NearToken::from_near(50)).await;
    let rpc = root_near.near().rpc();

    // No contract deployed yet
    let err = rpc
        .view_code(&account_id, Default::default())
        .await
        .unwrap_err();
    assert!(err.is_contract_not_deployed(), "unexpected error: {err:?}");

    let wasm_code = load_test_contract();
    account_near
        .transaction(account_near.account_id())
        .add_action(Action::deploy_contract(wasm_code.clone()))
        .await
        .unwrap();

    let contract = rpc
        .view_code(&account_id, Default::default())
        .await
        .unwrap();
    assert_eq!(contract.code, wasm_code);
    assert_eq!(contract.hash, CryptoHash::hash(&wasm_code));
}
