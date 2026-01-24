//! Meta-Transactions (NEP-366 Delegate Actions)
//!
//! Gasless transactions where a relayer pays the gas fee for a user's signed actions.
//! Perfect for onboarding users without requiring them to hold NEAR for gas.
//!
//! Run: cargo run --example meta_transactions
//!
//! Environment variables:
//!   USER_ACCOUNT_ID / USER_PRIVATE_KEY - The user who signs (doesn't pay gas)
//!   RELAYER_ACCOUNT_ID / RELAYER_PRIVATE_KEY - The relayer who submits (pays gas)

use near_kit::*;

// ============================================================================
// User Side: Create and sign delegate action (off-chain, no gas cost)
// ============================================================================

async fn user_creates_delegate(
    user_near: &Near,
    user_account: &str,
) -> Result<DelegateResult, Error> {
    println!("=== User Side ===\n");

    // Build the transaction and sign it off-chain with .delegate()
    // The user pays NO gas - they're just signing the intent
    let delegate_result = user_near
        .transaction("guestbook.near-examples.testnet")
        .call("add_message")
        .args(serde_json::json!({ "text": "Gasless transaction from near-kit-rs!" }))
        .gas(Gas::tgas(30))
        .delegate(DelegateOptions::default())
        .await?;

    println!("User signed delegate action (no gas paid)");
    println!("Sender: {user_account}");
    println!("Payload length: {} bytes", delegate_result.payload.len());

    // In a real app, the user would send this payload to the relayer via HTTP:
    //
    // reqwest::Client::new()
    //     .post("https://relayer.example.com/relay")
    //     .json(&serde_json::json!({ "payload": delegate_result.payload }))
    //     .send()
    //     .await?;

    Ok(delegate_result)
}

// ============================================================================
// Relayer Side: Submit delegate action to blockchain (pays gas)
// ============================================================================

async fn relayer_submits_delegate(
    relayer_near: &Near,
    delegate_result: DelegateResult,
) -> Result<FinalExecutionOutcome, Error> {
    println!("\n=== Relayer Side ===\n");

    // Relayer decodes and wraps the user's signed action
    // The relayer pays gas, but the contract sees the USER as the signer
    let outcome = relayer_near
        .transaction(delegate_result.sender_id())
        .signed_delegate_action(delegate_result.signed_delegate_action)
        .send()
        .await?;

    println!(
        "Relayer submitted transaction: {:?}",
        outcome.transaction_hash()
    );
    println!("Gas paid by: relayer");

    Ok(outcome)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Meta-Transactions Example\n");
    println!("This demonstrates gasless transactions where a relayer pays for a user.\n");

    // Get credentials from environment
    let user_account =
        std::env::var("USER_ACCOUNT_ID").unwrap_or_else(|_| "user.testnet".to_string());
    let user_key = std::env::var("USER_PRIVATE_KEY").ok();

    let relayer_account =
        std::env::var("RELAYER_ACCOUNT_ID").unwrap_or_else(|_| "relayer.testnet".to_string());
    let relayer_key = std::env::var("RELAYER_PRIVATE_KEY").ok();

    match (user_key, relayer_key) {
        (Some(user_key), Some(relayer_key)) => {
            // Set up both clients
            let user_near = Near::testnet()
                .credentials(&user_key, &user_account)?
                .build();

            let relayer_near = Near::testnet()
                .credentials(&relayer_key, &relayer_account)?
                .build();

            // Step 1: User signs the action off-chain
            let delegate_result = user_creates_delegate(&user_near, &user_account).await?;

            // Step 2: Relayer submits it on-chain (pays gas)
            relayer_submits_delegate(&relayer_near, delegate_result).await?;

            println!("\n=== Result ===");
            println!("User's action executed without paying gas!");
        }
        _ => {
            println!("To run this example, set environment variables:\n");
            println!("  USER_ACCOUNT_ID=user.testnet");
            println!("  USER_PRIVATE_KEY=ed25519:...");
            println!("  RELAYER_ACCOUNT_ID=relayer.testnet");
            println!("  RELAYER_PRIVATE_KEY=ed25519:...\n");
            println!("The user signs the action, the relayer pays for gas.");
        }
    }

    Ok(())
}
