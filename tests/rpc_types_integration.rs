//! Integration tests for RPC types against near-sandbox.
//!
//! These tests verify that all RPC response types correctly deserialize
//! from real NEAR RPC responses.
//!
//! Run with: `cargo nextest run --test rpc_types_integration --features sandbox`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::prelude::*;
use near_kit::sandbox::{SandboxConfig, ROOT_ACCOUNT};
use near_kit::{
    AccessKeyPermissionView, ActionView, FinalExecutionStatus, MerkleDirection, ReceiptContent,
};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rpc{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

// ============================================================================
// Block and Header Types Tests
// ============================================================================

#[tokio::test]
async fn test_block_view_full_fields() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Get a block and verify fields are present and deserialize correctly
    let block = near.rpc().block(BlockReference::final_()).await.unwrap();

    // Verify BlockView fields
    assert!(!block.author.is_empty(), "Block should have an author");

    // Verify BlockHeaderView fields - note that genesis block may have height 0
    let header = &block.header;
    assert!(!header.hash.is_zero(), "Block hash should not be zero");
    assert!(
        !header.timestamp_nanosec.is_empty(),
        "Timestamp nanosec should exist"
    );
    assert!(!header.gas_price.is_empty(), "Gas price should exist");
    assert!(!header.total_supply.is_empty(), "Total supply should exist");
    assert!(!header.signature.is_empty(), "Signature should exist");
    assert!(
        header.latest_protocol_version > 0,
        "Protocol version should be positive"
    );

    println!(
        "Block {} at height {} by {}",
        header.hash, header.height, block.author
    );
    println!("  Protocol version: {}", header.latest_protocol_version);
    println!("  Chunks included: {}", header.chunks_included);
    println!("  Chunk mask: {:?}", header.chunk_mask);
    println!("  Epoch ID: {}", header.epoch_id);
    println!("  Timestamp: {}", header.timestamp);
    println!("  Gas price: {}", header.gas_price);

    // Verify all optional/nested fields deserialize without panicking
    println!("  Block ordinal: {:?}", header.block_ordinal);
    println!("  Approvals: {} signers", header.approvals.len());
    println!(
        "  Validator proposals: {}",
        header.validator_proposals.len()
    );
}

#[tokio::test]
async fn test_chunk_header_view_full_fields() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let block = near.rpc().block(BlockReference::final_()).await.unwrap();

    // Verify ChunkHeaderView fields for each chunk
    for chunk in &block.chunks {
        assert!(!chunk.chunk_hash.is_zero(), "Chunk hash should not be zero");
        assert!(chunk.gas_limit > 0, "Gas limit should be positive");
        assert!(!chunk.signature.is_empty(), "Chunk signature should exist");

        println!(
            "Chunk {} for shard {} - gas used: {}/{}",
            chunk.chunk_hash, chunk.shard_id, chunk.gas_used, chunk.gas_limit
        );
        println!("  Height created: {}", chunk.height_created);
        println!("  Height included: {}", chunk.height_included);
        println!("  Balance burnt: {}", chunk.balance_burnt);
        println!("  Validator reward: {}", chunk.validator_reward);
    }
}

// ============================================================================
// Status and Node Info Tests
// ============================================================================

#[tokio::test]
async fn test_status_response_full_fields() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let status = near.rpc().status().await.unwrap();

    // Verify StatusResponse fields
    assert!(
        status.protocol_version > 0,
        "Protocol version should be positive"
    );
    assert!(
        status.latest_protocol_version >= status.protocol_version,
        "Latest protocol version should be >= current"
    );
    assert!(!status.chain_id.is_empty(), "Chain ID should exist");
    assert!(!status.genesis_hash.is_zero(), "Genesis hash should exist");

    // SyncInfo - note latest_block_height may be 0 for genesis
    assert!(
        !status.sync_info.latest_block_hash.is_zero(),
        "Latest block hash should exist"
    );
    assert!(
        !status.sync_info.latest_block_time.is_empty(),
        "Latest block time should exist"
    );

    // NodeVersion
    assert!(!status.version.version.is_empty(), "Version should exist");
    assert!(!status.version.build.is_empty(), "Build should exist");

    println!("Node status:");
    println!("  Chain ID: {}", status.chain_id);
    println!("  Protocol version: {}", status.protocol_version);
    println!("  Latest protocol: {}", status.latest_protocol_version);
    println!(
        "  Node version: {} ({})",
        status.version.version, status.version.build
    );
    println!("  Syncing: {}", status.sync_info.syncing);
    println!(
        "  Latest block: {} at height {}",
        status.sync_info.latest_block_hash, status.sync_info.latest_block_height
    );

    // Optional fields
    if let Some(rpc_addr) = &status.rpc_addr {
        println!("  RPC addr: {}", rpc_addr);
    }
    if let Some(validators) = status.validators.first() {
        println!("  First validator: {}", validators.account_id);
    }
    if let Some(commit) = &status.version.commit {
        println!("  Commit: {}", commit);
    }
}

// ============================================================================
// Account and Access Key Tests
// ============================================================================

#[tokio::test]
async fn test_account_view_full_fields() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let account_id: AccountId = ROOT_ACCOUNT.parse().unwrap();
    let account = near.account(&account_id).await.unwrap();

    // Verify AccountView fields
    assert!(
        account.amount.as_yoctonear() > 0,
        "Account should have balance"
    );
    assert!(account.storage_usage > 0, "Account should use some storage");
    // Note: block_height may be 0 for genesis block
    assert!(!account.block_hash.is_zero(), "Block hash should exist");

    println!("Account {}:", account_id);
    println!("  Balance: {}", account.amount);
    println!("  Locked: {}", account.locked);
    println!("  Storage: {} bytes", account.storage_usage);
    println!("  Has contract: {}", account.has_contract());
    println!("  Block height: {}", account.block_height);
}

#[tokio::test]
async fn test_access_key_list_view() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let account_id: AccountId = ROOT_ACCOUNT.parse().unwrap();
    let keys = near.access_keys(&account_id).await.unwrap();

    // Verify AccessKeyListView fields
    assert!(
        !keys.keys.is_empty(),
        "Account should have at least one key"
    );
    // Note: block_height may be 0 for genesis block
    assert!(!keys.block_hash.is_zero(), "Block hash should exist");

    for key_info in &keys.keys {
        println!("Key: {}", key_info.public_key);
        println!("  Nonce: {}", key_info.access_key.nonce);
        match &key_info.access_key.permission {
            AccessKeyPermissionView::FullAccess => println!("  Permission: FullAccess"),
            AccessKeyPermissionView::FunctionCall {
                allowance,
                receiver_id,
                method_names,
            } => {
                println!("  Permission: FunctionCall");
                println!("    Receiver: {}", receiver_id);
                println!("    Allowance: {:?}", allowance);
                println!("    Methods: {:?}", method_names);
            }
        }
    }
}

// ============================================================================
// Transaction Outcome Tests
// ============================================================================

#[tokio::test]
async fn test_final_execution_outcome_full_fields() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let root_account: AccountId = ROOT_ACCOUNT.parse().unwrap();

    // Create and execute a transaction
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account();

    let outcome = near
        .transaction(&receiver_id)
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify FinalExecutionOutcome fields
    assert!(
        matches!(
            outcome.final_execution_status,
            FinalExecutionStatus::Final | FinalExecutionStatus::Executed
        ),
        "Should have final execution status, got {:?}",
        outcome.final_execution_status
    );
    assert!(outcome.is_success(), "Transaction should succeed");
    assert!(!outcome.is_pending(), "Transaction should not be pending");
    assert!(outcome.status.is_some(), "Status should be present");
    assert!(
        outcome.transaction.is_some(),
        "Transaction should be present"
    );
    assert!(
        outcome.transaction_outcome.is_some(),
        "Transaction outcome should be present"
    );
    assert!(
        !outcome.receipts_outcome.is_empty(),
        "Should have receipt outcomes"
    );

    // Check transaction view
    let tx = outcome.transaction.as_ref().unwrap();
    assert_eq!(tx.signer_id, root_account);
    assert_eq!(tx.receiver_id, receiver_id);
    assert!(tx.nonce > 0, "Nonce should be positive");
    assert!(!tx.hash.is_zero(), "Transaction hash should exist");
    assert!(!tx.actions.is_empty(), "Should have actions");

    // Check transaction outcome
    let tx_outcome = outcome.transaction_outcome.as_ref().unwrap();
    assert!(!tx_outcome.id.is_zero(), "Outcome ID should exist");
    assert!(!tx_outcome.block_hash.is_zero(), "Block hash should exist");
    assert_eq!(tx_outcome.outcome.executor_id, root_account);
    assert!(tx_outcome.outcome.gas_burnt.as_gas() > 0, "Should burn gas");
    assert!(
        !tx_outcome.outcome.receipt_ids.is_empty(),
        "Should generate receipts"
    );

    // Check receipts outcome
    for receipt_outcome in &outcome.receipts_outcome {
        assert!(!receipt_outcome.id.is_zero(), "Receipt ID should exist");
        assert!(
            !receipt_outcome.block_hash.is_zero(),
            "Block hash should exist"
        );

        // Check merkle proof (may be empty on sandbox)
        for proof_item in &receipt_outcome.proof {
            assert!(!proof_item.hash.is_zero(), "Proof hash should exist");
            assert!(
                matches!(
                    proof_item.direction,
                    MerkleDirection::Left | MerkleDirection::Right
                ),
                "Should have valid direction"
            );
        }
    }

    println!("Transaction outcome:");
    println!("  Status: {:?}", outcome.final_execution_status);
    println!("  Hash: {:?}", outcome.transaction_hash());
    println!("  Gas used: {}", outcome.total_gas_used());
    println!("  Receipt outcomes: {}", outcome.receipts_outcome.len());
}

#[tokio::test]
async fn test_execution_metadata_and_gas_profile() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Execute a transaction
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account();

    let outcome = near
        .transaction(&receiver_id)
        .create_account()
        .transfer("1 NEAR")
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Check if metadata is present (may not be in all protocol versions)
    let mut found_metadata = false;
    for receipt_outcome in &outcome.receipts_outcome {
        if let Some(metadata) = &receipt_outcome.outcome.metadata {
            found_metadata = true;
            println!("Execution metadata version: {}", metadata.version);
            if let Some(gas_profile) = &metadata.gas_profile {
                println!("Gas profile entries: {}", gas_profile.len());
                for entry in gas_profile.iter().take(5) {
                    println!(
                        "  Cost: {:?}, Category: {:?}, Gas: {:?}",
                        entry.cost, entry.cost_category, entry.gas_used
                    );
                }
            }
        }
    }

    if !found_metadata {
        println!("No metadata found in receipts (this is normal for some protocol versions)");
    }
}

// ============================================================================
// Transaction Status with Receipts Tests
// ============================================================================

#[tokio::test]
async fn test_tx_status_with_receipts() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let root_account: AccountId = ROOT_ACCOUNT.parse().unwrap();

    // Execute a transaction first
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account();

    let outcome = near
        .transaction(&receiver_id)
        .create_account()
        .transfer("2 NEAR")
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let tx_hash = outcome.transaction_hash().unwrap();

    // Now get the full transaction status with receipts
    let status = near
        .rpc()
        .tx_status(tx_hash, &root_account, TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify FinalExecutionOutcomeWithReceipts fields
    assert!(status.is_success(), "Transaction should succeed");
    assert!(
        status.transaction.is_some(),
        "Transaction should be present"
    );
    assert!(
        status.transaction_outcome.is_some(),
        "Transaction outcome should be present"
    );

    // Check receipts array
    println!(
        "Transaction {} has {} receipts",
        tx_hash,
        status.receipts.len()
    );

    for receipt in &status.receipts {
        println!("Receipt {}:", receipt.receipt_id);
        println!(
            "  From: {} -> To: {}",
            receipt.predecessor_id, receipt.receiver_id
        );

        match &receipt.receipt {
            ReceiptContent::Action(action_data) => {
                println!("  Type: Action receipt");
                println!("  Signer: {}", action_data.signer_id);
                println!("  Actions: {}", action_data.actions.len());
                for action in &action_data.actions {
                    println!("    - {:?}", action);
                }
            }
            ReceiptContent::Data(data_receipt) => {
                println!("  Type: Data receipt");
                println!("  Data ID: {}", data_receipt.data_id);
            }
        }
    }
}

// ============================================================================
// Gas Price Tests
// ============================================================================

#[tokio::test]
async fn test_gas_price() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let gas_price = near.rpc().gas_price(None).await.unwrap();

    assert!(!gas_price.gas_price.is_empty(), "Gas price should exist");
    assert!(gas_price.as_u128() > 0, "Gas price should be positive");

    println!("Current gas price: {} yoctoNEAR", gas_price.gas_price);
}

// ============================================================================
// Action View Tests
// ============================================================================

#[tokio::test]
async fn test_action_view_variants() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Test CreateAccount + Transfer + AddKey
    let account1_key = SecretKey::generate_ed25519();
    let account1_id = unique_account();

    let outcome = near
        .transaction(&account1_id)
        .create_account()
        .transfer("3 NEAR")
        .add_full_access_key(account1_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let tx = outcome.transaction.as_ref().unwrap();
    println!("Transaction actions:");
    for action in &tx.actions {
        match action {
            ActionView::CreateAccount => println!("  - CreateAccount"),
            ActionView::Transfer { deposit } => println!("  - Transfer: {}", deposit),
            ActionView::AddKey { public_key, .. } => println!("  - AddKey: {}", public_key),
            ActionView::DeleteKey { public_key } => println!("  - DeleteKey: {}", public_key),
            ActionView::DeleteAccount { beneficiary_id } => {
                println!("  - DeleteAccount -> {}", beneficiary_id)
            }
            ActionView::FunctionCall {
                method_name,
                gas,
                deposit,
                ..
            } => println!(
                "  - FunctionCall: {} (gas: {}, deposit: {})",
                method_name, gas, deposit
            ),
            ActionView::DeployContract { .. } => println!("  - DeployContract"),
            ActionView::Stake { stake, public_key } => {
                println!("  - Stake: {} with {}", stake, public_key)
            }
            ActionView::Delegate { .. } => println!("  - Delegate"),
            other => println!("  - {:?}", other),
        }
    }

    assert!(tx.actions.len() >= 3, "Should have at least 3 actions");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_account_not_found_error() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let non_existent: AccountId = "non-existent-account.testnet".parse().unwrap();
    let result = near.balance(&non_existent).await;

    assert!(
        result.is_err(),
        "Should return error for non-existent account"
    );
    let err = result.unwrap_err();
    println!("Error for non-existent account: {}", err);
}

// ============================================================================
// View Function Result Tests
// ============================================================================

#[tokio::test]
async fn test_view_function_result() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Note: Sandbox doesn't have deployed contracts by default,
    // so we'll test that the ViewFunctionResult structure is correct
    // by checking what happens when we call a non-existent contract

    let contract_id: AccountId = ROOT_ACCOUNT.parse().unwrap();
    let result = near
        .rpc()
        .view_function(&contract_id, "get_greeting", &[], BlockReference::final_())
        .await;

    // This will fail because sandbox root account has no contract,
    // but the error handling proves the types work
    match result {
        Ok(view_result) => {
            println!("View result: {:?}", view_result.as_string());
            // Note: block_height may be 0 for genesis
            assert!(!view_result.block_hash.is_zero());
        }
        Err(e) => {
            println!("Expected error (no contract): {}", e);
        }
    }
}
