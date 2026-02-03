//! Debug tests that print actual RPC responses from sandbox.
//!
//! These tests are useful for inspecting actual response structures.
//!
//! Run with: `cargo test --test debug_rpc_responses --features sandbox -- --nocapture`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;
use near_kit::{ActionView, ReceiptContent};

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a unique subaccount ID for test isolation
fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("debug{}.{}", n, ROOT_ACCOUNT).parse().unwrap()
}

/// A minimal valid WASM module for testing
fn get_test_contract_wasm() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ]
}

#[tokio::test]
async fn debug_sync_info_and_status() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let status = near.rpc().status().await.unwrap();

    println!("\n========================================");
    println!("NODE STATUS");
    println!("========================================");
    println!("Chain ID: {}", status.chain_id);
    println!("Protocol version: {}", status.protocol_version);
    println!(
        "Latest protocol version: {}",
        status.latest_protocol_version
    );
    println!("Genesis hash: {}", status.genesis_hash);

    println!("\n--- Node Version ---");
    println!("Version: {}", status.version.version);
    println!("Build: {}", status.version.build);
    println!("Commit: {:?}", status.version.commit);
    println!("Rustc version: {:?}", status.version.rustc_version);

    println!("\n--- Sync Info ---");
    println!("Latest block hash: {}", status.sync_info.latest_block_hash);
    println!(
        "Latest block height: {}",
        status.sync_info.latest_block_height
    );
    println!("Latest block time: {}", status.sync_info.latest_block_time);
    println!("Syncing: {}", status.sync_info.syncing);
    println!(
        "Latest state root: {:?}",
        status.sync_info.latest_state_root
    );
    println!(
        "Earliest block hash: {:?}",
        status.sync_info.earliest_block_hash
    );
    println!(
        "Earliest block height: {:?}",
        status.sync_info.earliest_block_height
    );
    println!(
        "Earliest block time: {:?}",
        status.sync_info.earliest_block_time
    );
    println!("Epoch ID: {:?}", status.sync_info.epoch_id);
    println!(
        "Epoch start height: {:?}",
        status.sync_info.epoch_start_height
    );

    println!("\n--- Optional Fields ---");
    println!("RPC addr: {:?}", status.rpc_addr);
    println!("Node public key: {:?}", status.node_public_key);
    println!("Node key: {:?}", status.node_key);
    println!("Validator account ID: {:?}", status.validator_account_id);
    println!("Validator public key: {:?}", status.validator_public_key);
    println!("Uptime sec: {:?}", status.uptime_sec);

    println!("\n--- Validators ({}) ---", status.validators.len());
    for (i, v) in status.validators.iter().enumerate() {
        println!("  [{}] {}", i, v.account_id);
    }
}

#[tokio::test]
async fn debug_block_details() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let block = near.rpc().block(BlockReference::final_()).await.unwrap();

    println!("\n========================================");
    println!("BLOCK DETAILS");
    println!("========================================");
    println!("Author: {}", block.author);

    let h = &block.header;
    println!("\n--- Header ---");
    println!("Height: {}", h.height);
    println!("Hash: {}", h.hash);
    println!("Prev hash: {}", h.prev_hash);
    println!("Prev height: {:?}", h.prev_height);
    println!("Timestamp: {}", h.timestamp);
    println!("Timestamp nanosec: {}", h.timestamp_nanosec);
    println!("Epoch ID: {}", h.epoch_id);
    println!("Next epoch ID: {}", h.next_epoch_id);
    println!("Gas price: {}", h.gas_price);
    println!("Total supply: {}", h.total_supply);
    println!("Latest protocol version: {}", h.latest_protocol_version);
    println!("Chunks included: {}", h.chunks_included);
    println!("Chunk mask: {:?}", h.chunk_mask);
    println!("Block ordinal: {:?}", h.block_ordinal);
    println!("Approvals count: {}", h.approvals.len());
    println!("Signature: {}", h.signature);

    println!("\n--- Chunks ({}) ---", block.chunks.len());
    for (i, c) in block.chunks.iter().enumerate() {
        println!("\n  Chunk [{}]:", i);
        println!("    Chunk hash: {}", c.chunk_hash);
        println!("    Shard ID: {}", c.shard_id);
        println!("    Height created: {}", c.height_created);
        println!("    Height included: {}", c.height_included);
        println!("    Gas used/limit: {}/{}", c.gas_used, c.gas_limit);
        println!("    Balance burnt: {}", c.balance_burnt);
        println!("    Validator reward: {}", c.validator_reward);
        println!("    Encoded length: {}", c.encoded_length);
    }
}

#[tokio::test]
async fn debug_transaction_receipts() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    let root_account: AccountId = ROOT_ACCOUNT.parse().unwrap();

    // Create an account with multiple actions to generate receipts
    let receiver_key = SecretKey::generate_ed25519();
    let receiver_id = unique_account();

    let outcome = near
        .transaction(&receiver_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(receiver_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("\n========================================");
    println!("TRANSACTION OUTCOME");
    println!("========================================");
    println!(
        "Final execution status: {:?}",
        outcome.final_execution_status
    );
    println!("Is success: {}", outcome.is_success());
    println!("Is pending: {}", outcome.is_pending());
    println!("Status: {:?}", outcome.status);

    if let Some(tx) = &outcome.transaction {
        println!("\n--- Transaction ---");
        println!("Hash: {}", tx.hash);
        println!("Signer: {}", tx.signer_id);
        println!("Receiver: {}", tx.receiver_id);
        println!("Nonce: {}", tx.nonce);
        println!("Public key: {}", tx.public_key);
        println!("Priority fee: {:?}", tx.priority_fee);
        println!("Signature: {:?}", tx.signature);
        println!("Actions ({}):", tx.actions.len());
        for (i, action) in tx.actions.iter().enumerate() {
            println!("  [{}] {:?}", i, action);
        }
    }

    if let Some(tx_outcome) = &outcome.transaction_outcome {
        println!("\n--- Transaction Outcome ---");
        println!("ID: {}", tx_outcome.id);
        println!("Block hash: {}", tx_outcome.block_hash);
        println!("Executor: {}", tx_outcome.outcome.executor_id);
        println!("Gas burnt: {}", tx_outcome.outcome.gas_burnt);
        println!("Tokens burnt: {}", tx_outcome.outcome.tokens_burnt);
        println!("Logs: {:?}", tx_outcome.outcome.logs);
        println!("Receipt IDs: {:?}", tx_outcome.outcome.receipt_ids);
        println!("Status: {:?}", tx_outcome.outcome.status);
        println!("Proof items: {}", tx_outcome.proof.len());

        if let Some(metadata) = &tx_outcome.outcome.metadata {
            println!("Metadata version: {}", metadata.version);
            if let Some(gas_profile) = &metadata.gas_profile {
                println!("Gas profile ({} entries):", gas_profile.len());
                for entry in gas_profile.iter().take(5) {
                    println!(
                        "  - {:?}: {:?} ({:?})",
                        entry.cost, entry.gas_used, entry.cost_category
                    );
                }
            }
        }
    }

    println!(
        "\n--- Receipt Outcomes ({}) ---",
        outcome.receipts_outcome.len()
    );
    for (i, ro) in outcome.receipts_outcome.iter().enumerate() {
        println!("\n  Receipt Outcome [{}]:", i);
        println!("    ID: {}", ro.id);
        println!("    Block hash: {}", ro.block_hash);
        println!("    Executor: {}", ro.outcome.executor_id);
        println!("    Gas burnt: {}", ro.outcome.gas_burnt);
        println!("    Tokens burnt: {}", ro.outcome.tokens_burnt);
        println!("    Logs: {:?}", ro.outcome.logs);
        println!("    Receipt IDs generated: {:?}", ro.outcome.receipt_ids);
        println!("    Status: {:?}", ro.outcome.status);
        println!("    Proof items: {}", ro.proof.len());
    }

    // Now get full receipt details via EXPERIMENTAL_tx_status
    let tx_hash = outcome.transaction_hash().unwrap();
    let full_status = near
        .rpc()
        .tx_status(tx_hash, &root_account, TxExecutionStatus::Final)
        .await
        .unwrap();

    println!("\n========================================");
    println!("FULL RECEIPTS (via EXPERIMENTAL_tx_status)");
    println!("========================================");
    println!("Receipt count: {}", full_status.receipts.len());

    for (i, receipt) in full_status.receipts.iter().enumerate() {
        println!("\n  Receipt [{}]:", i);
        println!("    Receipt ID: {}", receipt.receipt_id);
        println!("    Predecessor: {}", receipt.predecessor_id);
        println!("    Receiver: {}", receipt.receiver_id);
        println!("    Priority: {:?}", receipt.priority);

        match &receipt.receipt {
            ReceiptContent::Action(action_data) => {
                println!("    Type: ACTION");
                println!("    Signer: {}", action_data.signer_id);
                println!("    Signer public key: {}", action_data.signer_public_key);
                println!("    Gas price: {}", action_data.gas_price);
                println!("    Is promise yield: {:?}", action_data.is_promise_yield);
                println!("    Input data IDs: {:?}", action_data.input_data_ids);
                println!(
                    "    Output data receivers: {:?}",
                    action_data.output_data_receivers
                );
                println!("    Actions ({}):", action_data.actions.len());
                for (j, action) in action_data.actions.iter().enumerate() {
                    match action {
                        ActionView::CreateAccount => println!("      [{}] CreateAccount", j),
                        ActionView::Transfer { deposit } => {
                            println!("      [{}] Transfer: {}", j, deposit)
                        }
                        ActionView::AddKey { public_key, .. } => {
                            println!("      [{}] AddKey: {}", j, public_key)
                        }
                        ActionView::FunctionCall {
                            method_name,
                            gas,
                            deposit,
                            args,
                        } => {
                            println!(
                                "      [{}] FunctionCall: {} (gas: {}, deposit: {}, args_len: {})",
                                j,
                                method_name,
                                gas,
                                deposit,
                                args.len()
                            )
                        }
                        other => println!("      [{}] {:?}", j, other),
                    }
                }
            }
            ReceiptContent::Data(data_receipt) => {
                println!("    Type: DATA");
                println!("    Data ID: {}", data_receipt.data_id);
                println!("    Data: {:?}", data_receipt.data);
            }
        }
    }

    // Silence unused warning
    let _ = rpc_url;
}

#[tokio::test]
async fn debug_access_key_details() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let root_account: AccountId = ROOT_ACCOUNT.parse().unwrap();

    // Create account with function call key
    let account_key = SecretKey::generate_ed25519();
    let fc_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Create account and add a function call key
    near.transaction(&account_id)
        .create_account()
        .transfer(NearToken::near(5))
        .add_full_access_key(account_key.public_key())
        .add_function_call_key(
            fc_key.public_key(),
            &account_id, // receiver
            vec!["get_greeting".to_string(), "set_greeting".to_string()],
            Some(NearToken::near(1)), // allowance
        )
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let keys = near.access_keys(&account_id).await.unwrap();

    println!("\n========================================");
    println!("ACCESS KEYS");
    println!("========================================");
    println!("Block height: {}", keys.block_height);
    println!("Block hash: {}", keys.block_hash);
    println!("Keys count: {}", keys.keys.len());

    for (i, key_info) in keys.keys.iter().enumerate() {
        println!("\n  Key [{}]:", i);
        println!("    Public key: {}", key_info.public_key);
        println!("    Nonce: {}", key_info.access_key.nonce);
        match &key_info.access_key.permission {
            near_kit::AccessKeyPermissionView::FullAccess => {
                println!("    Permission: FullAccess");
            }
            near_kit::AccessKeyPermissionView::FunctionCall {
                allowance,
                receiver_id,
                method_names,
            } => {
                println!("    Permission: FunctionCall");
                println!("      Receiver: {}", receiver_id);
                println!("      Allowance: {:?}", allowance);
                println!("      Method names: {:?}", method_names);
            }
        }
    }

    // Silence unused warning
    let _ = root_account;
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[tokio::test]
async fn test_error_account_not_found() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let non_existent: AccountId = "this-account-does-not-exist.near".parse().unwrap();
    let result = near.balance(&non_existent).await;

    println!("\n--- AccountNotFound Error ---");
    match result {
        Err(e) => {
            println!("Error: {}", e);
        }
        Ok(_) => panic!("Expected error for non-existent account"),
    }
}

#[tokio::test]
async fn test_error_contract_not_deployed() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let account_id: AccountId = ROOT_ACCOUNT.parse().unwrap();

    // Try to call a function on an account without a contract
    let result = near
        .rpc()
        .view_function(&account_id, "get_greeting", &[], BlockReference::final_())
        .await;

    println!("\n--- ContractNotDeployed Error ---");
    match result {
        Err(e) => {
            println!("Error: {}", e);
        }
        Ok(r) => {
            println!("Unexpected success: {:?}", r.as_string());
        }
    }
}

#[tokio::test]
async fn test_error_unknown_block() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Try to get a block that doesn't exist (very high block number)
    let result = near.rpc().block(BlockReference::at_height(999999999)).await;

    println!("\n--- UnknownBlock Error ---");
    match result {
        Err(e) => {
            println!("Error: {}", e);
        }
        Ok(_) => panic!("Expected error for non-existent block"),
    }
}

#[tokio::test]
async fn test_error_invalid_method() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Create an account and deploy a minimal contract
    let contract_key = SecretKey::generate_ed25519();
    let contract_id = unique_account();

    let wasm = get_test_contract_wasm();
    near.transaction(&contract_id)
        .create_account()
        .transfer(NearToken::near(10))
        .add_full_access_key(contract_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Try to call a method that doesn't exist
    let result = near
        .rpc()
        .view_function(
            &contract_id,
            "nonexistent_method",
            &[],
            BlockReference::final_(),
        )
        .await;

    println!("\n--- Method Not Found Error ---");
    match result {
        Err(e) => {
            println!("Error: {}", e);
        }
        Ok(r) => {
            println!("Result (might be contract error): {:?}", r.as_string());
        }
    }
}
