//! High-Throughput Transactions with RotatingSigner
//!
//! Demonstrates using multiple access keys per account to send many concurrent
//! transactions without nonce collisions.
//!
//! Run: cargo run --example rotating_signer --features sandbox
//!
//! This example requires the `sandbox` feature and will start a local NEAR node.

use near_kit::*;

#[cfg(feature = "sandbox")]
use near_kit::sandbox::{OwnedSandbox, SandboxConfig};

#[cfg(feature = "sandbox")]
async fn high_throughput_example() -> Result<(), Error> {
    println!("Starting local sandbox...\n");
    let sandbox: OwnedSandbox = SandboxConfig::fresh().await;

    let root_near = sandbox.client();
    let root_account = root_near.account_id().unwrap().to_string();

    // Generate 5 keypairs dynamically
    let num_keys = 5;
    let keypairs: Vec<KeyPair> = (0..num_keys).map(|_| KeyPair::random()).collect();

    // Create a bot account with the first key
    let bot_account = format!("bot-{}.{}", std::process::id(), root_account);
    println!("Creating bot account: {bot_account}");

    root_near
        .transaction(&bot_account)
        .create_account()
        .transfer(NearToken::near(50))
        .add_full_access_key(keypairs[0].public_key.clone())
        .send()
        .await?;

    // Add remaining keys using a loop
    let bot_near = Near::custom(sandbox.rpc_url())
        .signer(InMemorySigner::from_secret_key(
            bot_account.parse()?,
            keypairs[0].secret_key.clone(),
        ))
        .build();

    println!("Adding {} more access keys...", num_keys - 1);

    // Dynamic key addition - fold over keypairs to build transaction
    keypairs[1..]
        .iter()
        .fold(bot_near.transaction(&bot_account), |tx, kp| {
            tx.add_full_access_key(kp.public_key.clone())
        })
        .send()
        .await?;

    // Create RotatingSigner with all keys
    let secret_keys: Vec<SecretKey> = keypairs.into_iter().map(|kp| kp.secret_key).collect();
    let rotating_signer = RotatingSigner::new(&bot_account, secret_keys)?;

    let near = Near::custom(sandbox.rpc_url())
        .signer(rotating_signer)
        .build();

    // Create recipient accounts
    let num_recipients = 20;
    println!("\nCreating {num_recipients} recipient accounts...");

    for i in 0..num_recipients {
        let recipient = format!("recipient-{i}.{root_account}");
        root_near
            .transaction(&recipient)
            .create_account()
            .transfer(NearToken::millinear(100))
            .send()
            .await?;
    }

    // Send concurrent transfers - RotatingSigner prevents nonce collisions!
    println!("\nSending {num_recipients} concurrent transfers...");
    let start = std::time::Instant::now();

    let root_account_clone = root_account.clone();
    let futures: Vec<_> = (0..num_recipients)
        .map(|i| {
            let recipient = format!("recipient-{i}.{root_account_clone}");
            let near = near.clone();
            async move { near.transfer(&recipient, NearToken::near(1)).await }
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
        let recipient = format!("recipient-{i}.{root_account}");
        let balance = near.balance(&recipient).await?;
        println!("  {recipient}: {}", balance.available);
    }

    println!("\nDone.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("RotatingSigner Example\n");
    println!("Demonstrates high-throughput concurrent transactions using multiple access keys.\n");

    #[cfg(feature = "sandbox")]
    {
        high_throughput_example().await?;
    }

    #[cfg(not(feature = "sandbox"))]
    {
        println!("This example requires the `sandbox` feature.");
        println!("Run with: cargo run --example rotating_signer --features sandbox");
    }

    Ok(())
}
