//! High-Throughput Transactions with RotatingSigner
//!
//! Demonstrates using multiple access keys per account to send many concurrent
//! transactions without nonce collisions.
//!
//! Run: cargo run -p near-kit-sandbox --example rotating_signer --features integration-tests
//!
//! This example starts a local NEAR node through Docker.

use near_kit::protocol::Action;
use near_kit::*;

use near_kit::rpc::FinalExecutionOutcome;
use near_kit::signer::{InMemorySigner, RotatingSigner, SecretKey};
use near_kit_sandbox::{Sandbox, SandboxConfig};

async fn high_throughput_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting local sandbox...\n");
    let sandbox: Sandbox = SandboxConfig::fresh().await?;

    let root_near = sandbox.client();
    let root_account = root_near.account_id().to_string();

    // Generate 5 keys dynamically
    let num_keys = 5;
    let secret_keys: Vec<SecretKey> = (0..num_keys)
        .map(|_| SecretKey::generate_ed25519())
        .collect();

    // Create a bot account with the first key
    let bot_account: AccountId = format!("bot-{}.{}", std::process::id(), root_account).parse()?;
    println!("Creating bot account: {bot_account}");

    root_near
        .transaction(&bot_account)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(secret_keys[0].public_key()))
        .await?;

    // Add remaining keys using a loop
    let bot_near = sandbox
        .client()
        .near()
        .with_signer(InMemorySigner::from_secret_key(
            &bot_account,
            secret_keys[0].clone(),
        )?);

    println!("Adding {} more access keys...", num_keys - 1);

    // Dynamic key addition - fold over keys to build transaction
    secret_keys[1..]
        .iter()
        .fold(bot_near.transaction(&bot_account), |tx, key| {
            tx.add_action(Action::add_full_access_key(key.public_key()))
        })
        .await?;

    // Create RotatingSigner with all keys
    let rotating_signer = RotatingSigner::new(&bot_account, secret_keys)?;

    let near = sandbox.client().near().with_signer(rotating_signer);

    // Create recipient accounts
    let num_recipients = 20;
    println!("\nCreating {num_recipients} recipient accounts...");

    for i in 0..num_recipients {
        let recipient: AccountId = format!("recipient-{i}.{root_account}").parse()?;
        root_near
            .transaction(&recipient)
            .add_action(Action::create_account())
            .add_action(Action::transfer(NearToken::from_millinear(100)))
            .await?;
    }

    // Send concurrent transfers - RotatingSigner prevents nonce collisions!
    println!("\nSending {num_recipients} concurrent transfers...");
    let start = std::time::Instant::now();

    let root_account_clone = root_account.clone();
    let futures: Vec<_> = (0..num_recipients)
        .map(|i| {
            let recipient: AccountId = format!("recipient-{i}.{root_account_clone}")
                .parse()
                .expect("valid recipient account ID");
            let near = near.clone();
            async move {
                near.transaction(&recipient)
                    .add_action(Action::transfer(NearToken::from_near(1)))
                    .await
            }
        })
        .collect();

    let results: Vec<Result<FinalExecutionOutcome, Error>> =
        futures::future::join_all(futures).await;
    let duration = start.elapsed();

    let succeeded = results.iter().filter(|r| r.is_ok()).count();
    let failed = results.iter().filter(|r| r.is_err()).count();

    println!("\n=== Results ===");
    println!("Succeeded: {succeeded}/{num_recipients}");
    println!("Failed:    {failed}/{num_recipients}");
    println!("Duration:  {:.2?}", duration);
    println!(
        "Throughput: {:.1} tx/s",
        succeeded as f64 / duration.as_secs_f64()
    );

    // Show a sample of failures if any
    for (i, result) in results.iter().enumerate() {
        if let Err(e) = result {
            println!("\nTransaction {i} failed: {e}");
            break;
        }
    }

    // Verify some balances
    println!("\nSample recipient balances:");
    for i in 0..3 {
        let recipient: AccountId = format!("recipient-{i}.{root_account}").parse()?;
        let balance = near.near().balance(&recipient).await?;
        println!("  {recipient}: {}", balance.available);
    }

    println!("\nDone.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("RotatingSigner Example\n");
    println!("Demonstrates high-throughput concurrent transactions using multiple access keys.\n");

    high_throughput_example().await?;

    Ok(())
}
