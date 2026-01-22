//! Integration tests running against near-sandbox.
//!
//! These tests use a shared sandbox instance with unique subaccounts per test,
//! following the pattern from defuse-sandbox.
//!
//! Run with: `cargo nextest run --test sandbox_integration --features sandbox`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use rstest::{fixture, rstest};

/// Counter for generating unique subaccount names
static SUB_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Test context with isolated subaccount
pub struct TestContext {
    /// Near client configured with this test's subaccount
    pub near: Near,
    /// The subaccount ID for this test (e.g., "test0.sandbox")
    pub account_id: AccountId,
    /// Secret key for the subaccount
    pub secret_key: SecretKey,
    /// Root account ID (for beneficiary in delete_account, etc.)
    pub root_account: AccountId,
    /// RPC URL (for creating additional clients)
    pub rpc_url: String,
}

impl TestContext {
    /// Create a Near client with custom credentials (e.g., for a newly created account)
    pub fn client_for(&self, secret_key: &SecretKey, account_id: &AccountId) -> Near {
        Near::custom(&self.rpc_url)
            .credentials(secret_key.to_string(), account_id.as_str())
            .unwrap()
            .build()
    }
}

/// Fixture that provides an isolated test context with its own subaccount.
///
/// Each test gets a unique subaccount (e.g., "test0.sandbox", "test1.sandbox")
/// funded with 1000 NEAR. Tests cannot interfere with each other because they
/// can't sign transactions for accounts outside their namespace.
#[fixture]
pub async fn ctx() -> TestContext {
    // Get shared sandbox using the new API
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let root_account: AccountId = ROOT_ACCOUNT.parse().unwrap();
    let rpc_url = sandbox.rpc_url().to_string();

    // Generate unique subaccount name
    let test_num = SUB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let subaccount_id: AccountId = format!("test{}.{}", test_num, root_account)
        .parse()
        .unwrap();

    // Generate key for the subaccount
    let subaccount_key = SecretKey::generate_ed25519();

    // Create the subaccount using the root account
    root_near
        .transaction(&subaccount_id)
        .create_account()
        .transfer("1000 NEAR")
        .add_full_access_key(subaccount_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create client for the subaccount
    let near = Near::custom(&rpc_url)
        .credentials(subaccount_key.to_string(), subaccount_id.as_str())
        .unwrap()
        .build();

    TestContext {
        near,
        account_id: subaccount_id,
        secret_key: subaccount_key,
        root_account,
        rpc_url,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_sandbox_balance(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    let balance = ctx.near.balance(&ctx.account_id).await.unwrap();
    println!("Test account balance: {}", balance);

    // Should have approximately 1000 NEAR (minus account creation costs)
    assert!(balance.total > NearToken::from_near(999));
}

#[rstest]
#[tokio::test]
async fn test_sandbox_transfer(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Generate a new keypair for the receiver
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", ctx.account_id).parse().unwrap();

    // Create the receiver account
    let outcome = ctx
        .near
        .transaction(&receiver_id)
        .create_account()
        .transfer("10 NEAR")
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!(
        "Create account outcome: success={}, hash={:?}",
        outcome.is_success(),
        outcome.transaction_hash()
    );

    // Check the receiver's balance
    let balance = ctx.near.balance(&receiver_id).await.unwrap();
    println!("Receiver balance: {}", balance);

    // Should have approximately 10 NEAR (minus storage costs)
    assert!(balance.total > NearToken::from_near(9));
    assert!(balance.total < NearToken::from_near(11));
}

#[rstest]
#[tokio::test]
async fn test_sandbox_multiple_transfers(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Generate keypairs for multiple receivers
    let receiver1_key = SecretKey::generate_ed25519();
    let receiver1_id: AccountId = format!("receiver1.{}", ctx.account_id).parse().unwrap();

    let receiver2_key = SecretKey::generate_ed25519();
    let receiver2_id: AccountId = format!("receiver2.{}", ctx.account_id).parse().unwrap();

    // Create first account
    ctx.near
        .transaction(&receiver1_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(receiver1_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create second account
    ctx.near
        .transaction(&receiver2_id)
        .create_account()
        .transfer("3 NEAR")
        .add_full_access_key(receiver2_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check balances
    let balance1 = ctx.near.balance(&receiver1_id).await.unwrap();
    let balance2 = ctx.near.balance(&receiver2_id).await.unwrap();

    println!("Receiver1 balance: {}", balance1);
    println!("Receiver2 balance: {}", balance2);

    assert!(balance1.total > NearToken::from_near(4));
    assert!(balance2.total > NearToken::from_near(2));
}

#[rstest]
#[tokio::test]
async fn test_sandbox_simple_transfer(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // First create a receiver account
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("bob.{}", ctx.account_id).parse().unwrap();

    ctx.near
        .transaction(&receiver_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let initial_balance = ctx.near.balance(&receiver_id).await.unwrap();

    // Now do a simple transfer using the convenience method
    ctx.near
        .transfer(&receiver_id, "2 NEAR")
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let final_balance = ctx.near.balance(&receiver_id).await.unwrap();

    println!("Initial: {}, Final: {}", initial_balance, final_balance);

    // Balance should have increased by ~2 NEAR
    let diff = final_balance.total.as_yoctonear() - initial_balance.total.as_yoctonear();
    let expected = NearToken::from_near(2).as_yoctonear();
    assert!(diff == expected, "Expected +2 NEAR, got diff: {}", diff);
}

#[rstest]
#[tokio::test]
async fn test_sandbox_create_account_outcome(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Create a contract account
    let contract_key = SecretKey::generate_ed25519();
    let contract_id: AccountId = format!("contract.{}", ctx.account_id).parse().unwrap();

    // Create account with funding
    let outcome = ctx
        .near
        .transaction(&contract_id)
        .create_account()
        .transfer("50 NEAR")
        .add_full_access_key(contract_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("Transaction hash: {:?}", outcome.transaction_hash());
    println!("Gas used: {}", outcome.total_gas_used());

    assert!(outcome.is_success());
}

#[rstest]
#[tokio::test]
async fn test_sandbox_delete_account(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Create an account to delete
    let temp_key = SecretKey::generate_ed25519();
    let temp_id: AccountId = format!("temporary.{}", ctx.account_id).parse().unwrap();

    ctx.near
        .transaction(&temp_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(temp_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify it exists
    assert!(ctx.near.account_exists(&temp_id).await.unwrap());

    // Create a new client with the temp account's key to delete it
    let temp_near = ctx.client_for(&temp_key, &temp_id);

    // Delete the account, sending remaining balance to test account
    temp_near
        .transaction(&temp_id)
        .delete_account(&ctx.account_id)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify it no longer exists
    assert!(!ctx.near.account_exists(&temp_id).await.unwrap());
}

#[rstest]
#[tokio::test]
async fn test_sandbox_add_and_delete_key(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Create an account
    let account_key = SecretKey::generate_ed25519();
    let account_id: AccountId = format!("keytest.{}", ctx.account_id).parse().unwrap();

    ctx.near
        .transaction(&account_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a new client for this account
    let account_near = ctx.client_for(&account_key, &account_id);

    // Add a second key
    let second_key = SecretKey::generate_ed25519();

    account_near
        .transaction(&account_id)
        .add_full_access_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check that both keys exist
    let keys = account_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 2);

    // Delete the second key
    account_near
        .transaction(&account_id)
        .delete_key(second_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check that only one key remains
    let keys = account_near.access_keys(&account_id).await.unwrap();
    assert_eq!(keys.keys.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_sandbox_multiple_actions_in_one_transaction(#[future] ctx: TestContext) {
    let ctx = ctx.await;

    // Create two accounts in separate transactions first
    let alice_key = SecretKey::generate_ed25519();
    let alice_id: AccountId = format!("alice.{}", ctx.account_id).parse().unwrap();

    let bob_key = SecretKey::generate_ed25519();
    let bob_id: AccountId = format!("bob.{}", ctx.account_id).parse().unwrap();

    // Create alice
    ctx.near
        .transaction(&alice_id)
        .create_account()
        .transfer("20 NEAR")
        .add_full_access_key(alice_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create bob
    ctx.near
        .transaction(&bob_id)
        .create_account()
        .transfer("10 NEAR")
        .add_full_access_key(bob_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify both exist
    assert!(ctx.near.account_exists(&alice_id).await.unwrap());
    assert!(ctx.near.account_exists(&bob_id).await.unwrap());

    let alice_balance = ctx.near.balance(&alice_id).await.unwrap();
    let bob_balance = ctx.near.balance(&bob_id).await.unwrap();

    println!("Alice: {}, Bob: {}", alice_balance, bob_balance);

    assert!(alice_balance.total > NearToken::from_near(19));
    assert!(bob_balance.total > NearToken::from_near(9));
}
