//! Integration tests for error handling against the sandbox.
//!
//! These tests verify that error paths work correctly with real NEAR RPC responses.
//! Run with: `cargo test --features sandbox --test integration error_handling`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("err{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// Account Not Found Errors
// =============================================================================

#[tokio::test]
async fn test_error_balance_nonexistent_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let nonexistent: AccountId = "definitely-does-not-exist-12345.sandbox".parse().unwrap();
    let result = near.balance(&nonexistent).await;

    assert!(
        result.is_err(),
        "Should return error for non-existent account"
    );
    let err = result.unwrap_err();

    // Should be an RPC error indicating account not found
    match err {
        Error::Rpc(rpc_err) => {
            assert!(
                matches!(rpc_err, RpcError::AccountNotFound(_)),
                "Expected AccountNotFound, got: {:?}",
                rpc_err
            );
        }
        other => panic!("Expected Rpc error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_error_account_info_nonexistent() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let nonexistent: AccountId = "nonexistent-account-xyz.sandbox".parse().unwrap();
    let result = near.account(&nonexistent).await;

    assert!(result.is_err());
    let err = result.unwrap_err();

    match err {
        Error::Rpc(RpcError::AccountNotFound(account_id)) => {
            assert_eq!(account_id.as_str(), "nonexistent-account-xyz.sandbox");
        }
        other => panic!("Expected AccountNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_access_keys_nonexistent_account_returns_empty() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Note: Unlike view_account, the NEAR RPC returns an empty list for
    // access_keys on non-existent accounts rather than an error
    let nonexistent: AccountId = "no-such-account.sandbox".parse().unwrap();
    let result = near.access_keys(&nonexistent).await.unwrap();

    assert!(
        result.keys.is_empty(),
        "Non-existent account should have no keys"
    );
}

#[tokio::test]
async fn test_account_exists_returns_false_for_nonexistent() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // This should NOT error - it should return false
    let exists = near
        .account_exists("definitely-not-real.sandbox")
        .await
        .unwrap();

    assert!(
        !exists,
        "Non-existent account should return false, not error"
    );
}

// =============================================================================
// Contract Not Deployed Errors
// =============================================================================

#[tokio::test]
async fn test_error_view_on_account_without_contract() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Root account exists but has no contract
    let result: Result<serde_json::Value, _> =
        near.view(ROOT_ACCOUNT, "get_greeting").args(()).await;

    assert!(
        result.is_err(),
        "Should error when calling view on non-contract"
    );
    let err = result.unwrap_err();

    match err {
        Error::Rpc(RpcError::ContractNotDeployed(account_id)) => {
            assert!(account_id.as_str().contains("sandbox"));
        }
        Error::Rpc(RpcError::ContractExecution { .. }) => {
            // Some versions return this instead
        }
        other => panic!(
            "Expected ContractNotDeployed or ContractExecution, got: {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn test_error_view_on_newly_created_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create a new account without a contract
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to call a view method on the account
    let result: Result<serde_json::Value, _> = near.view(&account_id, "some_method").args(()).await;

    assert!(
        result.is_err(),
        "Should error when calling view on account without contract"
    );
}

// =============================================================================
// Method Not Found / Contract Execution Errors
// =============================================================================

#[tokio::test]
async fn test_error_view_nonexistent_method() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy a contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Call a method that doesn't exist
    let result: Result<serde_json::Value, _> = near
        .view(&contract_id, "this_method_does_not_exist")
        .args(serde_json::json!({}))
        .await;

    assert!(
        result.is_err(),
        "Should error when calling non-existent method"
    );
    let err = result.unwrap_err();

    match err {
        Error::Rpc(RpcError::ContractExecution {
            contract_id: cid,
            method_name,
            message,
        }) => {
            assert_eq!(cid.as_str(), contract_id.as_str());
            assert!(method_name.is_some());
            assert!(
                message.contains("MethodNotFound") || message.contains("MethodResolveError"),
                "Expected method not found error, got: {}",
                message
            );
        }
        other => panic!("Expected ContractExecution error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_error_view_with_invalid_args() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Deploy guestbook contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Call with invalid JSON args (not valid for the method)
    let result: Result<Vec<serde_json::Value>, _> = near
        .view(&contract_id, "get_messages")
        .args("not valid json args") // This should cause issues
        .await;

    // The behavior depends on the contract, but it should handle gracefully
    // Either it succeeds (some contracts ignore extra args) or fails with a clear error
    println!("Result with invalid args: {:?}", result);
}

// =============================================================================
// Transaction Errors
// =============================================================================

#[tokio::test]
async fn test_error_transfer_to_nonexistent_implicit_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create a sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    near.transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Transfer to a non-existent named account (not implicit)
    // This should work because the receiver doesn't need to exist for named accounts
    // (funds just go there and create an implicit record)
    let nonexistent_named: AccountId = "receiver.nonexistent.sandbox".parse().unwrap();

    // Note: transferring to a truly non-existent account may or may not fail
    // depending on NEAR protocol rules. Let's test creating a subaccount that doesn't exist.
    let result = sender_near
        .transfer(&nonexistent_named, NearToken::near(1))
        .await;

    // This should fail because the parent "nonexistent.sandbox" doesn't exist
    println!("Transfer to non-existent result: {:?}", result);
}

#[tokio::test]
async fn test_error_insufficient_balance_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender with small balance
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    near.transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create receiver
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("receiver.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::millinear(100))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to transfer more than available
    let result = sender_near
        .transfer(&receiver_id, NearToken::near(1000))
        .await;

    assert!(result.is_err(), "Should fail with insufficient balance");
    println!("Insufficient balance error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_error_create_account_that_already_exists() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create an account first
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Create a signer for the parent
    let parent_near = Near::custom(rpc_url)
        .credentials(sandbox.root_secret_key(), ROOT_ACCOUNT)
        .unwrap()
        .build();

    // Try to create the same account again
    let new_key = SecretKey::generate_ed25519();
    let result = parent_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(new_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(
        result.is_err(),
        "Should fail when creating duplicate account"
    );
    println!("Duplicate account error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_error_delete_nonexistent_key() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create an account
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let account_near = Near::custom(rpc_url)
        .credentials(account_key.to_string(), account_id.as_str())
        .unwrap()
        .build();

    // Try to delete a key that doesn't exist on the account
    let fake_key = SecretKey::generate_ed25519();
    let result = account_near
        .transaction(&account_id)
        .delete_key(fake_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await;

    assert!(
        result.is_err(),
        "Should fail when deleting non-existent key"
    );
    println!("Delete non-existent key error: {:?}", result.unwrap_err());
}

// =============================================================================
// Function Call Errors
// =============================================================================

#[tokio::test]
async fn test_error_function_call_panic() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Deploy guestbook contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let contract_near = Near::custom(rpc_url)
        .credentials(contract_key.to_string(), contract_id.as_str())
        .unwrap()
        .build();

    // Call a method that doesn't exist (should fail during execution)
    let result = contract_near
        .call(&contract_id, "nonexistent_method")
        .args(serde_json::json!({}))
        .gas(Gas::tgas(30))
        .await;

    assert!(
        result.is_err(),
        "Should fail when calling non-existent method"
    );
    println!("Function call error: {:?}", result.unwrap_err());
}

#[tokio::test]
async fn test_error_function_call_insufficient_gas() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Deploy guestbook contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = std::fs::read("tests/contracts/guestbook.wasm").expect("guestbook.wasm not found");

    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let contract_near = Near::custom(rpc_url)
        .credentials(contract_key.to_string(), contract_id.as_str())
        .unwrap()
        .build();

    // Call with very low gas (should fail)
    let result = contract_near
        .call(&contract_id, "add_message")
        .args(serde_json::json!({ "text": "test" }))
        .gas(Gas::from_gas(1000)) // Extremely low gas
        .await;

    assert!(result.is_err(), "Should fail with insufficient gas");
    println!("Insufficient gas error: {:?}", result.unwrap_err());
}

// =============================================================================
// Block Reference Errors
// =============================================================================

#[tokio::test]
async fn test_error_query_at_nonexistent_block_height() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to query at a block height that doesn't exist yet
    let result = near.balance(ROOT_ACCOUNT).at_block(999999999999).await;

    assert!(result.is_err(), "Should fail for non-existent block height");
    let err = result.unwrap_err();

    match err {
        Error::Rpc(RpcError::UnknownBlock(_)) => {
            // Expected
        }
        other => {
            // Some implementations might return a different error
            println!("Got error: {:?}", other);
        }
    }
}

#[tokio::test]
async fn test_error_query_at_invalid_block_hash() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create an invalid block hash (all zeros won't exist)
    let fake_hash = CryptoHash::ZERO;

    let result = near.balance(ROOT_ACCOUNT).at_block_hash(fake_hash).await;

    assert!(result.is_err(), "Should fail for non-existent block hash");
    println!("Invalid block hash error: {:?}", result.unwrap_err());
}

// =============================================================================
// No Signer Errors
// =============================================================================

#[tokio::test]
async fn test_error_transaction_without_signer() {
    let sandbox = SandboxConfig::shared().await;
    let rpc_url = sandbox.rpc_url();

    // Create a Near client without a signer
    let near = Near::custom(rpc_url).build();

    // Try to send a transaction
    let receiver: AccountId = "some-receiver.sandbox".parse().unwrap();
    let result = near.transfer(&receiver, NearToken::near(1)).await;

    assert!(result.is_err(), "Should fail without signer");
    let err = result.unwrap_err();

    match err {
        Error::NoSigner => {
            // Expected
        }
        Error::NoSignerAccount => {
            // Also acceptable
        }
        other => panic!("Expected NoSigner or NoSignerAccount, got: {:?}", other),
    }
}

// =============================================================================
// RPC Status and Node Info Tests
// =============================================================================

#[tokio::test]
async fn test_rpc_status_returns_valid_data() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let status = near.rpc().status().await.unwrap();

    // Verify essential fields
    assert!(!status.chain_id.is_empty(), "Chain ID should be present");
    assert!(
        status.protocol_version > 0,
        "Protocol version should be positive"
    );
    assert!(
        !status.sync_info.latest_block_hash.is_zero(),
        "Block hash should exist"
    );
    assert!(
        !status.version.version.is_empty(),
        "Node version should be present"
    );

    println!(
        "Chain: {}, Protocol: {}",
        status.chain_id, status.protocol_version
    );
}

#[tokio::test]
async fn test_gas_price_returns_valid_data() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let gas_price = near.rpc().gas_price(None).await.unwrap();

    assert!(gas_price.as_u128() > 0, "Gas price should be positive");
    println!("Gas price: {} yoctoNEAR per gas", gas_price.gas_price);
}

#[tokio::test]
async fn test_block_query_returns_valid_data() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let block = near.rpc().block(BlockReference::final_()).await.unwrap();

    assert!(!block.author.is_empty(), "Block author should exist");
    assert!(!block.header.hash.is_zero(), "Block hash should exist");
    assert!(block.header.latest_protocol_version > 0);

    println!("Block {} by {}", block.header.hash, block.author);
}

// =============================================================================
// Finality Tests
// =============================================================================

#[tokio::test]
async fn test_query_with_different_finalities() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Query with Final finality
    let balance_final = near
        .balance(ROOT_ACCOUNT)
        .finality(Finality::Final)
        .await
        .unwrap();

    // Query with Optimistic finality
    let balance_optimistic = near
        .balance(ROOT_ACCOUNT)
        .finality(Finality::Optimistic)
        .await
        .unwrap();

    // Both should return valid balances
    assert!(balance_final.total.as_yoctonear() > 0);
    assert!(balance_optimistic.total.as_yoctonear() > 0);

    println!(
        "Final: {}, Optimistic: {}",
        balance_final, balance_optimistic
    );
}
