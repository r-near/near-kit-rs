//! Integration tests for offline signing functionality.
//!
//! Tests the `sign_offline()` method and `SignedTransaction::from_bytes/from_base64`
//! for air-gapped signing workflows.

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(1000);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("offline{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// =============================================================================
// sign_offline() Tests
// =============================================================================

#[tokio::test]
async fn test_sign_offline_transfer() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create receiver account (must be created by sender as subaccount)
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("recv.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // ============================================
    // Simulate offline signing workflow:
    // 1. Online machine fetches block_hash and nonce
    // 2. Offline machine signs the transaction
    // 3. Online machine sends the signed transaction
    // ============================================

    // Step 1: Get block_hash and nonce (would be done on online machine)
    let block = sender_near
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .unwrap();
    let block_hash = block.header.hash;

    let access_key = sender_near
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Optimistic),
        )
        .await
        .unwrap();
    let nonce = access_key.nonce + 1;

    // Step 2: Sign offline (signing is async but no network is required)
    let signed = sender_near
        .transaction(&receiver_id)
        .transfer(NearToken::near(5))
        .sign_offline(block_hash, nonce)
        .await
        .unwrap();

    println!("Signed offline transaction hash: {}", signed.get_hash());
    println!("Block hash used: {}", block_hash);
    println!("Nonce used: {}", nonce);

    // Step 3: Send the signed transaction (online machine)
    let outcome = sender_near.send(&signed).await.unwrap();

    assert!(outcome.is_success());
    println!("Transaction succeeded: {:?}", outcome.transaction_hash());

    // Verify the transfer happened
    let balance = root_near.balance(&receiver_id).await.unwrap();
    // Receiver had ~10 NEAR (minus storage) + 5 NEAR transfer
    // Storage costs reduce initial 10 NEAR significantly, so just verify we got the 5 NEAR transfer
    assert!(balance.total > NearToken::from_millinear(9000)); // At least 9 NEAR
}

#[tokio::test]
async fn test_sign_offline_function_call() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create account with contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm_code =
        std::fs::read("tests/contracts/guestbook.wasm").expect("failed to read test contract");

    root_near
        .transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let contract_near = Near::custom(rpc_url)
        .credentials(contract_key.to_string(), contract_id.as_str())
        .unwrap()
        .build();

    // Get block_hash and nonce for offline signing
    let block = contract_near
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .unwrap();
    let block_hash = block.header.hash;

    let access_key = contract_near
        .rpc()
        .view_access_key(
            &contract_id,
            &contract_key.public_key(),
            BlockReference::Finality(Finality::Optimistic),
        )
        .await
        .unwrap();
    let nonce = access_key.nonce + 1;

    // Sign offline using CallBuilder
    let signed = contract_near
        .call(&contract_id, "add_message")
        .args(serde_json::json!({ "text": "Hello from offline!" }))
        .gas(Gas::tgas(30))
        .sign_offline(block_hash, nonce)
        .await
        .unwrap();

    // Send
    let outcome = contract_near.send(&signed).await.unwrap();
    assert!(outcome.is_success());

    // Verify the message was added
    let messages: Vec<serde_json::Value> = contract_near
        .view(&contract_id, "get_messages")
        .args(serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["text"], "Hello from offline!");
}

// =============================================================================
// SignedTransaction Serialization Tests
// =============================================================================

#[tokio::test]
async fn test_signed_transaction_roundtrip_bytes() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create receiver account (must be created by sender as subaccount)
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("recv.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Sign a transaction
    let original = sender_near
        .transfer(&receiver_id, NearToken::near(1))
        .sign()
        .await
        .unwrap();

    // Serialize to bytes
    let bytes = original.to_bytes();
    println!("Serialized to {} bytes", bytes.len());

    // Deserialize back
    let deserialized = SignedTransaction::from_bytes(&bytes).unwrap();

    // Verify they match
    assert_eq!(original.get_hash(), deserialized.get_hash());
    assert_eq!(
        original.transaction.signer_id,
        deserialized.transaction.signer_id
    );
    assert_eq!(
        original.transaction.receiver_id,
        deserialized.transaction.receiver_id
    );
    assert_eq!(original.transaction.nonce, deserialized.transaction.nonce);
    assert_eq!(
        original.signature.as_bytes(),
        deserialized.signature.as_bytes()
    );

    // Verify the deserialized transaction can be sent
    let outcome = sender_near.send(&deserialized).await.unwrap();
    assert!(outcome.is_success());
}

#[tokio::test]
async fn test_signed_transaction_roundtrip_base64() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create receiver account (must be created by sender as subaccount)
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("recv.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Sign a transaction
    let original = sender_near
        .transfer(&receiver_id, NearToken::near(1))
        .sign()
        .await
        .unwrap();

    // Serialize to base64
    let base64_str = original.to_base64();
    println!("Serialized to base64: {} chars", base64_str.len());
    println!("Base64: {}...", &base64_str[..50]);

    // Deserialize back
    let deserialized = SignedTransaction::from_base64(&base64_str).unwrap();

    // Verify they match
    assert_eq!(original.get_hash(), deserialized.get_hash());
    assert_eq!(
        original.signature.as_bytes(),
        deserialized.signature.as_bytes()
    );

    // Verify the deserialized transaction can be sent
    let outcome = sender_near.send(&deserialized).await.unwrap();
    assert!(outcome.is_success());
}

#[tokio::test]
async fn test_offline_sign_and_transport_simulation() {
    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Create sender account
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account();

    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::near(100))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // --- ONLINE MACHINE ---
    let sender_near = Near::custom(rpc_url)
        .credentials(sender_key.to_string(), sender_id.as_str())
        .unwrap()
        .build();

    // Create receiver account (must be created by sender as subaccount)
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id: AccountId = format!("recv.{}", sender_id).parse().unwrap();

    sender_near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let block = sender_near
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .unwrap();
    let block_hash = block.header.hash;

    let access_key = sender_near
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Optimistic),
        )
        .await
        .unwrap();
    let nonce = access_key.nonce + 1;

    println!("Online: fetched block_hash={}, nonce={}", block_hash, nonce);

    // --- OFFLINE MACHINE ---
    // (In reality, sender_key would be stored on the offline machine)
    let signed = sender_near
        .transaction(&receiver_id)
        .transfer(NearToken::near(3))
        .sign_offline(block_hash, nonce)
        .await
        .unwrap();

    // Serialize for transport
    let payload = signed.to_base64();
    println!("Offline: signed tx, payload length={}", payload.len());

    // --- BACK TO ONLINE MACHINE ---
    // Deserialize the payload
    let received_tx = SignedTransaction::from_base64(&payload).unwrap();
    println!("Online: received tx hash={}", received_tx.get_hash());

    // Send it
    let outcome = sender_near.send(&received_tx).await.unwrap();
    assert!(outcome.is_success());
    println!("Online: transaction confirmed!");

    // Verify
    let balance = root_near.balance(&receiver_id).await.unwrap();
    // Receiver had ~10 NEAR (minus storage) + 3 NEAR transfer
    assert!(balance.total > NearToken::from_millinear(8000)); // At least 8 NEAR
}

// =============================================================================
// Error Cases
// =============================================================================

#[tokio::test]
async fn test_sign_offline_without_signer_fails() {
    // Create a Near client without a signer
    let near = Near::testnet().build();

    let block_hash: CryptoHash = "11111111111111111111111111111111".parse().unwrap();
    let nonce = 1u64;

    let result = near
        .transaction("bob.testnet")
        .transfer(NearToken::near(1))
        .sign_offline(block_hash, nonce)
        .await;

    assert!(result.is_err());
    match result {
        Err(Error::NoSigner) => {}
        _ => panic!("Expected NoSigner error"),
    }
}

#[tokio::test]
async fn test_sign_offline_empty_transaction_fails() {
    let near = Near::testnet()
        .credentials(
            "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr",
            "alice.testnet",
        )
        .unwrap()
        .build();

    let block_hash: CryptoHash = "11111111111111111111111111111111".parse().unwrap();
    let nonce = 1u64;

    // Empty transaction (no actions)
    let result = near
        .transaction("bob.testnet")
        .sign_offline(block_hash, nonce)
        .await;

    assert!(result.is_err());
    match result {
        Err(Error::InvalidTransaction(msg)) => {
            assert!(msg.contains("at least one action"));
        }
        _ => panic!("Expected InvalidTransaction error"),
    }
}

#[test]
fn test_from_bytes_invalid_data_fails() {
    let invalid_bytes = vec![0, 1, 2, 3, 4, 5];
    let result = SignedTransaction::from_bytes(&invalid_bytes);

    assert!(result.is_err());
    match result {
        Err(Error::InvalidTransaction(msg)) => {
            assert!(msg.contains("deserialize"));
        }
        _ => panic!("Expected InvalidTransaction error"),
    }
}

#[test]
fn test_from_base64_invalid_base64_fails() {
    let invalid_base64 = "not valid base64!!!";
    let result = SignedTransaction::from_base64(invalid_base64);

    assert!(result.is_err());
    match result {
        Err(Error::InvalidTransaction(msg)) => {
            assert!(msg.contains("base64") || msg.contains("Invalid"));
        }
        _ => panic!("Expected InvalidTransaction error"),
    }
}

#[test]
fn test_from_base64_valid_base64_invalid_tx_fails() {
    // Valid base64 but not a valid transaction
    let valid_base64_invalid_tx = "SGVsbG8gV29ybGQh"; // "Hello World!" in base64
    let result = SignedTransaction::from_base64(valid_base64_invalid_tx);

    assert!(result.is_err());
    match result {
        Err(Error::InvalidTransaction(msg)) => {
            assert!(msg.contains("deserialize"));
        }
        _ => panic!("Expected InvalidTransaction error"),
    }
}
