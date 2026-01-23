//! Integration tests for global contracts and all action types.
//!
//! These tests exercise the global contract actions (DeployGlobalContract, UseGlobalContract)
//! and NEP-616 deterministic account creation, as well as all other action types
//! supported by the TransactionBuilder.
//!
//! Run with: `cargo test --test global_contracts_integration --features sandbox`

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("gc{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

/// Load the test contract WASM
fn load_test_contract() -> Vec<u8> {
    std::fs::read("tests/contracts/guestbook.wasm").expect("failed to read test contract")
}

// =============================================================================
// Helper: Create a funded account
// =============================================================================

async fn create_funded_account(
    root_near: &Near,
    rpc_url: &str,
    funding: &str,
) -> (Near, AccountId, SecretKey) {
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(funding)
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let near = Near::custom(rpc_url)
        .credentials(account_key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    (near, account_id, account_key)
}

// =============================================================================
// Global Contract Tests
// =============================================================================

/// Test publishing a contract to the global registry by account ID (updatable).
#[tokio::test]
async fn test_publish_contract_by_account() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();

    // Publish the contract (by_hash = false means identified by account)
    let outcome = publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, false)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Publish contract succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());
}

/// Test publishing a contract to the global registry by code hash (immutable).
#[tokio::test]
async fn test_publish_contract_by_hash() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();

    // Publish the contract (by_hash = true means identified by code hash, immutable)
    let outcome = publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, true)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Publish contract by hash succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());
}

/// Test deploying a contract from a publisher account.
#[tokio::test]
async fn test_deploy_from_publisher() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();

    // First, publish the contract
    publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, false)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a user account that will deploy from the publisher
    let (user_near, user_id, _) = create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    // Deploy from the publisher's global contract
    let outcome = user_near
        .transaction(&user_id)
        .deploy_from_publisher(&publisher_id)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Deploy from publisher succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());

    // Verify the contract was deployed by checking account info
    let account = root_near.account(&user_id).await.unwrap();
    assert!(*account.code_hash.as_bytes() != [0u8; 32]);
}

/// Test deploying a contract from a code hash.
#[tokio::test]
async fn test_deploy_from_hash() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();

    // Calculate the code hash before publishing
    let code_hash = CryptoHash::hash(&wasm_code);
    println!("Contract code hash: {}", code_hash);

    // Publish the contract by hash (immutable)
    publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, true)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a user account that will deploy from the hash
    let (user_near, user_id, _) = create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    // Deploy from the code hash
    let outcome = user_near
        .transaction(&user_id)
        .deploy_from_hash(code_hash)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Deploy from hash succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());

    // Verify the contract was deployed
    let account = root_near.account(&user_id).await.unwrap();
    assert!(*account.code_hash.as_bytes() != [0u8; 32]);
}

/// Test NEP-616 deterministic state init with code hash.
#[tokio::test]
async fn test_state_init_by_hash() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();
    let code_hash = CryptoHash::hash(&wasm_code);

    // First, publish the contract by hash
    publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, true)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create initial state data (empty for this test)
    let initial_data: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

    // Use state_init to create a deterministic account
    // Note: The receiver_id doesn't matter for state_init - the account ID is derived
    let outcome = publisher_near
        .transaction(&publisher_id)
        .state_init_by_hash(code_hash, initial_data, "5 NEAR")
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "State init by hash succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());
}

/// Test NEP-616 deterministic state init with publisher account.
#[tokio::test]
async fn test_state_init_by_publisher() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a publisher account
    let (publisher_near, publisher_id, _) =
        create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let wasm_code = load_test_contract();

    // First, publish the contract by account
    publisher_near
        .transaction(&publisher_id)
        .publish_contract(wasm_code, false)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create initial state data with some sample data
    let mut initial_data: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    initial_data.insert(b"key1".to_vec(), b"value1".to_vec());

    // Use state_init to create a deterministic account
    let outcome = publisher_near
        .transaction(&publisher_id)
        .state_init_by_publisher(&publisher_id, initial_data, "5 NEAR")
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "State init by publisher succeeded: {:?}",
        outcome.transaction_hash()
    );
    assert!(outcome.is_success());
}

// =============================================================================
// Action Type Tests - Exercise all TransactionBuilder actions
// =============================================================================

/// Test CreateAccount action
#[tokio::test]
async fn test_action_create_account() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (parent_near, parent_id, _) = create_funded_account(&root_near, rpc_url, "20 NEAR").await;

    let child_key = SecretKey::generate_ed25519();
    let child_id: AccountId = format!("child.{}", parent_id).parse().unwrap();

    let outcome = parent_near
        .transaction(&child_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(child_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());
    assert!(root_near.account_exists(&child_id).await.unwrap());
}

/// Test Transfer action
#[tokio::test]
async fn test_action_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (sender_near, _, _) = create_funded_account(&root_near, rpc_url, "20 NEAR").await;
    let (_, receiver_id, _) = create_funded_account(&root_near, rpc_url, "5 NEAR").await;

    let initial_balance = root_near.balance(&receiver_id).await.unwrap();

    sender_near
        .transaction(&receiver_id)
        .transfer("3 NEAR")
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let final_balance = root_near.balance(&receiver_id).await.unwrap();
    let diff = final_balance.total.as_yoctonear() - initial_balance.total.as_yoctonear();

    assert_eq!(diff, NearToken::from_near(3).as_yoctonear());
}

/// Test DeployContract action
#[tokio::test]
async fn test_action_deploy_contract() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (contract_near, contract_id, _) =
        create_funded_account(&root_near, rpc_url, "20 NEAR").await;

    let wasm_code = load_test_contract();

    let outcome = contract_near
        .transaction(&contract_id)
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());

    // Verify contract was deployed
    let account = root_near.account(&contract_id).await.unwrap();
    assert!(*account.code_hash.as_bytes() != [0u8; 32]);
}

/// Test FunctionCall action
#[tokio::test]
async fn test_action_function_call() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (contract_near, contract_id, _) =
        create_funded_account(&root_near, rpc_url, "20 NEAR").await;

    let wasm_code = load_test_contract();

    // Deploy the contract
    contract_near
        .transaction(&contract_id)
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Call a method on the contract
    let outcome = contract_near
        .transaction(&contract_id)
        .call("add_message")
        .args(serde_json::json!({ "text": "Hello from test!" }))
        .gas("30 Tgas")
        .deposit("0 NEAR")
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());
}

/// Test AddKey action with full access
#[tokio::test]
async fn test_action_add_full_access_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (account_near, account_id, _) = create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    let new_key = SecretKey::generate_ed25519();

    let outcome = account_near
        .transaction(&account_id)
        .add_full_access_key(new_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());

    // Verify the key was added
    let keys = root_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 2);
}

/// Test AddKey action with function call access
#[tokio::test]
async fn test_action_add_function_call_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (account_near, account_id, _) = create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    let fc_key = SecretKey::generate_ed25519();
    let receiver_contract: AccountId = "some-contract.sandbox".parse().unwrap();

    let outcome = account_near
        .transaction(&account_id)
        .add_function_call_key(
            fc_key.public_key(),
            &receiver_contract,
            vec!["method1".to_string(), "method2".to_string()],
            Some(NearToken::from_near(1)),
        )
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());

    // Verify the key was added
    let keys = root_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 2);
}

/// Test DeleteKey action
#[tokio::test]
async fn test_action_delete_key() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (account_near, account_id, _) = create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    // Add a second key
    let second_key = SecretKey::generate_ed25519();

    account_near
        .transaction(&account_id)
        .add_full_access_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify 2 keys
    let keys = root_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 2);

    // Delete the second key
    account_near
        .transaction(&account_id)
        .delete_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify 1 key remains
    let keys = root_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 1);
}

/// Test DeleteAccount action
#[tokio::test]
async fn test_action_delete_account() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (beneficiary_near, beneficiary_id, _) =
        create_funded_account(&root_near, rpc_url, "10 NEAR").await;

    // Create an account to delete
    let to_delete_key = SecretKey::generate_ed25519();
    let to_delete_id: AccountId = format!("todelete.{}", beneficiary_id).parse().unwrap();

    beneficiary_near
        .transaction(&to_delete_id)
        .create_account()
        .transfer("2 NEAR")
        .add_full_access_key(to_delete_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(root_near.account_exists(&to_delete_id).await.unwrap());

    // Delete the account
    let to_delete_near = Near::custom(rpc_url)
        .credentials(to_delete_key.to_string(), to_delete_id.as_str())
        .unwrap()
        .build();

    to_delete_near
        .transaction(&to_delete_id)
        .delete_account(&beneficiary_id)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(!root_near.account_exists(&to_delete_id).await.unwrap());
}

/// Test Stake action
#[tokio::test]
async fn test_action_stake() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (staker_near, staker_id, staker_key) =
        create_funded_account(&root_near, rpc_url, "100 NEAR").await;

    // Stake action
    let outcome = staker_near
        .transaction(&staker_id)
        .stake("50 NEAR", staker_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("Stake action completed: {:?}", outcome.transaction_hash());
    assert!(outcome.is_success());

    // Verify account state reflects staking
    let account = root_near.account(&staker_id).await.unwrap();
    println!("Account locked balance: {}", account.locked);
}

/// Test multiple actions in a single transaction
#[tokio::test]
async fn test_multiple_actions() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (parent_near, parent_id, _) = create_funded_account(&root_near, rpc_url, "50 NEAR").await;

    let child_key = SecretKey::generate_ed25519();
    let child_id: AccountId = format!("multi.{}", parent_id).parse().unwrap();

    let wasm_code = load_test_contract();

    // Create account, fund it, add key, deploy contract - all in one tx
    let outcome = parent_near
        .transaction(&child_id)
        .create_account()
        .transfer("20 NEAR")
        .add_full_access_key(child_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());

    // Verify everything worked
    let account = root_near.account(&child_id).await.unwrap();
    assert!(*account.code_hash.as_bytes() != [0u8; 32]);

    let balance = root_near.balance(&child_id).await.unwrap();
    assert!(balance.total > NearToken::from_near(19));

    let keys = root_near.access_keys(&child_id).await.unwrap();
    assert_eq!(keys.keys.len(), 1);
}

/// Test chaining multiple function calls
#[tokio::test]
async fn test_multiple_function_calls() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let (contract_near, contract_id, _) =
        create_funded_account(&root_near, rpc_url, "20 NEAR").await;

    let wasm_code = load_test_contract();

    // Deploy the contract
    contract_near
        .transaction(&contract_id)
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Call multiple methods in one transaction
    let outcome = contract_near
        .transaction(&contract_id)
        .call("add_message")
        .args(serde_json::json!({ "text": "First message" }))
        .gas("15 Tgas")
        .call("add_message")
        .args(serde_json::json!({ "text": "Second message" }))
        .gas("15 Tgas")
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    assert!(outcome.is_success());
    println!(
        "Multiple function calls: gas used = {}",
        outcome.total_gas_used()
    );
}
