//! Typed Contracts - Compile-time safe contract interactions
//!
//! Shows how to define typed contract interfaces, compose standards via
//! trait inheritance, and build multi-action transactions with typed methods.
//!
//! Run: cargo run --example typed_contracts
//!
//! Set environment variables for write operations:
//!   NEAR_ACCOUNT_ID=your-account.testnet
//!   NEAR_PRIVATE_KEY=ed25519:...

use near_kit::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Define contract standards
// ============================================================================
//
// Standards like NEP-141 (fungible token) and NEP-145 (storage management)
// are reusable trait definitions. Define them once, reuse everywhere.

#[derive(Serialize)]
pub struct StorageDepositArgs {
    pub account_id: Option<String>,
    pub registration_only: Option<bool>,
}

/// NEP-145: Storage Management standard
#[near_kit::contract]
pub trait StorageManagement {
    #[call(payable)]
    fn storage_deposit(&mut self, args: StorageDepositArgs);
}

#[derive(Serialize)]
pub struct FtTransferArgs {
    pub receiver_id: String,
    pub amount: String,
    pub memo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FtMetadataView {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

/// NEP-141: Fungible Token standard
#[near_kit::contract]
pub trait Ft {
    fn ft_metadata(&self) -> FtMetadataView;

    #[call]
    fn ft_transfer(&mut self, args: FtTransferArgs);
}

// ============================================================================
// 2. Compose standards into a concrete contract
// ============================================================================
//
// Your contract implements multiple standards. Declare this with trait
// inheritance — the generated client automatically gets ALL methods.

#[derive(Serialize)]
pub struct MintArgs {
    pub account_id: String,
    pub amount: String,
}

/// A token contract that implements FT + storage management + custom methods
#[near_kit::contract]
pub trait MyToken: Ft + StorageManagement {
    #[call]
    fn mint(&mut self, args: MintArgs);
}

// ============================================================================
// 3. Simple contract calls (per-trait client)
// ============================================================================

async fn simple_calls(near: &Near) -> Result<(), Error> {
    println!("=== Simple Typed Calls ===\n");

    // Create a typed client — one object with ALL methods from ALL standards
    let token = near.contract::<dyn MyToken>("wrap.testnet");

    // View methods (from Ft standard)
    let metadata = token.ft_metadata().await?;
    println!("Token: {} ({})", metadata.name, metadata.symbol);

    // Change methods (from Ft standard)
    token
        .ft_transfer(FtTransferArgs {
            receiver_id: "bob.testnet".to_string(),
            amount: "1000000".to_string(),
            memo: Some("payment".to_string()),
        })
        .gas(Gas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .await?;

    // Change methods (from StorageManagement standard)
    token
        .storage_deposit(StorageDepositArgs {
            account_id: Some("bob.testnet".to_string()),
            registration_only: Some(true),
        })
        .deposit(NearToken::from_millinear(100))
        .await?;

    // Custom methods (from MyToken itself)
    token
        .mint(MintArgs {
            account_id: "alice.testnet".to_string(),
            amount: "5000000".to_string(),
        })
        .await?;

    Ok(())
}

// ============================================================================
// 4. Typed transaction composition
// ============================================================================
//
// Use `.typed_call::<dyn T>()` to compose multiple typed actions in a single
// atomic transaction. Mix with raw actions like state_init or transfer.

async fn composed_transaction(near: &Near) -> Result<(), Error> {
    println!("=== Composed Transaction ===\n");

    // Register storage + transfer tokens in a single atomic transaction
    let outcome = near
        .transaction("wrap.testnet")
        // First action: register storage for bob (typed, from StorageManagement)
        .typed_call::<dyn MyToken>()
        .storage_deposit(StorageDepositArgs {
            account_id: Some("bob.testnet".to_string()),
            registration_only: Some(true),
        })
        .deposit(NearToken::from_millinear(100))
        // Second action: transfer tokens to bob (typed, from Ft)
        .typed_call::<dyn MyToken>()
        .ft_transfer(FtTransferArgs {
            receiver_id: "bob.testnet".to_string(),
            amount: "1000000".to_string(),
            memo: None,
        })
        .deposit(NearToken::from_yoctonear(1))
        // Send as one atomic transaction
        .send()
        .await?;

    println!("Composed tx: {:?}", outcome.transaction_hash());
    println!("Gas used: {}", outcome.total_gas_used());

    Ok(())
}

// ============================================================================
// 5. Mix typed calls with raw actions
// ============================================================================

async fn mixed_transaction(near: &Near) -> Result<(), Error> {
    println!("=== Mixed Raw + Typed Transaction ===\n");

    let outcome = near
        .transaction("wrap.testnet")
        // Raw action: transfer NEAR
        .transfer(NearToken::from_millinear(10))
        // Typed action: register storage
        .typed_call::<dyn StorageManagement>()
        .storage_deposit(StorageDepositArgs {
            account_id: None,
            registration_only: None,
        })
        .deposit(NearToken::from_millinear(100))
        // Raw action via FunctionCall
        .add_action(
            FunctionCall::new("custom_method")
                .args(serde_json::json!({"key": "value"}))
                .gas(Gas::from_tgas(10)),
        )
        .send()
        .await?;

    println!("Mixed tx: {:?}", outcome.transaction_hash());

    Ok(())
}

// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Read-only operations work without credentials
    let near = Near::testnet().build();
    let token = near.contract::<dyn MyToken>("wrap.testnet");

    // View calls work without a signer
    match token.ft_metadata().await {
        Ok(meta) => println!(
            "Token: {} ({}), decimals: {}",
            meta.name, meta.symbol, meta.decimals
        ),
        Err(e) => println!("View call failed (expected on testnet): {e}"),
    }

    // Write operations need credentials
    let account_id = std::env::var("NEAR_ACCOUNT_ID").ok();
    let private_key = std::env::var("NEAR_PRIVATE_KEY").ok();

    match (account_id, private_key) {
        (Some(account), Some(key)) => {
            let near = Near::testnet().credentials(&key, &account)?.build();
            simple_calls(&near).await?;
            composed_transaction(&near).await?;
            mixed_transaction(&near).await?;
        }
        _ => {
            println!("\nSet NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY for write operations.");
        }
    }

    Ok(())
}
