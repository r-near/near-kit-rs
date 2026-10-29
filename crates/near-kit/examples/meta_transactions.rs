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

use near_kit::protocol::Action;
use near_kit::protocol::SignedDelegateAction;
use near_kit::rpc::FinalExecutionOutcome;
use near_kit::signer::InMemorySigner;
use near_kit::transaction::{DelegateOptions, FunctionCall};
use near_kit::*;

// ============================================================================
// User Side: Create and sign delegate action (off-chain, no gas cost)
// ============================================================================

async fn user_creates_delegate(user: &SigningNear) -> Result<SignedDelegateAction, Error> {
    println!("=== User Side ===\n");

    // Sign the actions off-chain with .delegate()
    // The user pays NO gas - they're just signing the intent
    let call = FunctionCall::new("add_message")
        .args(serde_json::json!({ "text": "Gasless transaction from near-kit-rs!" }))
        .gas(Gas::from_tgas(30));
    let delegate_result = user
        .delegate(
            account!("guestbook.near-examples.testnet"),
            vec![call.try_into()?],
            DelegateOptions::default(),
        )
        .await?;

    println!("User signed delegate action (no gas paid)");
    println!("Sender: {}", user.account_id());
    println!(
        "Payload length: {} bytes",
        delegate_result.to_base64().len()
    );

    // In a real app, the user would send this payload to the relayer via HTTP:
    //
    // reqwest::Client::new()
    //     .post("https://relayer.example.com/relay")
    //     .json(&serde_json::json!({ "payload": delegate_result.to_base64() }))
    //     .send()
    //     .await?;

    Ok(delegate_result)
}

// ============================================================================
// Relayer Side: Submit delegate action to blockchain (pays gas)
// ============================================================================

async fn relayer_submits_delegate(
    relayer: &SigningNear,
    delegate_result: SignedDelegateAction,
) -> Result<FinalExecutionOutcome, Error> {
    println!("\n=== Relayer Side ===\n");

    // Relayer decodes and wraps the user's signed action
    // The relayer pays gas, but the contract sees the USER as the signer
    // The transaction goes to the user's account, the delegate's sender.
    let outcome = relayer
        .transaction(delegate_result.sender_id())
        .add_action(Action::delegate(delegate_result))
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
            let near = Near::testnet().build();
            let user = near.with_signer(InMemorySigner::new(
                user_account.parse::<AccountId>()?,
                user_key.parse()?,
            ));
            let relayer = near.with_signer(InMemorySigner::new(
                relayer_account.parse::<AccountId>()?,
                relayer_key.parse()?,
            ));

            // Step 1: User signs the action off-chain
            let delegate_result = user_creates_delegate(&user).await?;

            // Step 2: Relayer submits it on-chain (pays gas)
            relayer_submits_delegate(&relayer, delegate_result).await?;

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
