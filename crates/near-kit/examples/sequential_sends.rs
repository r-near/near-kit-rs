//! Sequential Per-Key Transaction Sends
//!
//! Demonstrates how to build a per-key sequential send pattern using
//! `RotatingSigner::into_per_key_signers()`. This ensures transactions
//! from the same access key are sent one at a time, waiting for block
//! inclusion before sending the next — preventing the send-ordering
//! race condition where a higher nonce arrives at a validator before
//! a lower one.
//!
//! Run: cargo run --example sequential_sends --features sandbox
//!
//! This example requires the `sandbox` feature and will start a local NEAR node.

use near_kit::*;

#[cfg(feature = "sandbox")]
use near_kit::sandbox::{OwnedSandbox, SandboxConfig};

#[cfg(feature = "sandbox")]
async fn sequential_example() -> Result<(), Error> {
    println!("Starting local sandbox...\n");
    let sandbox: OwnedSandbox = SandboxConfig::fresh().await;

    let root_near = sandbox.client();
    let root_account = root_near.account_id().unwrap().to_string();

    // Generate 3 keypairs for the bot account
    let num_keys = 3;
    let keypairs: Vec<KeyPair> = (0..num_keys).map(|_| KeyPair::random()).collect();

    // Create a bot account with the first key
    let bot_account = format!("bot-{}.{}", std::process::id(), root_account);
    println!("Creating bot account: {bot_account}");

    root_near
        .transaction(&bot_account)
        .create_account()
        .transfer(NearToken::from_near(50))
        .add_full_access_key(keypairs[0].public_key.clone())
        .send()
        .await?;

    // Add remaining keys
    let bot_near = Near::custom(sandbox.rpc_url())
        .signer(InMemorySigner::from_secret_key(
            bot_account.parse()?,
            keypairs[0].secret_key.clone(),
        ))
        .build();

    keypairs[1..]
        .iter()
        .fold(bot_near.transaction(&bot_account), |tx, kp| {
            tx.add_full_access_key(kp.public_key.clone())
        })
        .send()
        .await?;

    // Create recipient
    let recipient = format!("recipient.{root_account}");
    root_near
        .transaction(&recipient)
        .create_account()
        .transfer(NearToken::from_millinear(100))
        .send()
        .await?;

    // Split RotatingSigner into per-key signers
    let secret_keys: Vec<SecretKey> = keypairs.into_iter().map(|kp| kp.secret_key).collect();
    let rotating = RotatingSigner::new(&bot_account, secret_keys)?;

    println!(
        "Splitting {} keys into per-key signers...",
        rotating.key_count()
    );
    let per_key_signers = rotating.into_per_key_signers();

    // Each key gets its own sequential queue via tokio::spawn.
    // Within each queue, transactions wait for block inclusion before
    // sending the next one — preventing nonce ordering issues.
    let txs_per_key = 5;
    println!("Sending {txs_per_key} sequential txs per key ({num_keys} keys in parallel)...\n");

    let start = std::time::Instant::now();
    let rpc_url = sandbox.rpc_url().to_string();

    let handles: Vec<_> = per_key_signers
        .into_iter()
        .enumerate()
        .map(|(key_idx, signer)| {
            let rpc_url = rpc_url.clone();
            let recipient = recipient.clone();
            tokio::spawn(async move {
                let near = Near::custom(&rpc_url).signer(signer).build();
                for tx_idx in 0..txs_per_key {
                    near.transfer(&recipient, NearToken::from_millinear(1))
                        .send()
                        .wait_until(TxExecutionStatus::Included)
                        .await
                        .unwrap();
                    println!("  key[{key_idx}] tx {tx_idx} included");
                }
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }

    let duration = start.elapsed();
    let total = num_keys * txs_per_key;

    println!("\n=== Results ===");
    println!("Total txs:  {total}");
    println!("Duration:   {:.2?}", duration);
    println!(
        "Throughput: {:.1} tx/s",
        total as f64 / duration.as_secs_f64()
    );

    // Verify balance
    let balance = bot_near.balance(&recipient).await?;
    println!("\nRecipient balance: {}", balance.available);

    println!("\nDone.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Sequential Per-Key Sends Example\n");
    println!(
        "Demonstrates per-key sequential transaction execution using into_per_key_signers().\n"
    );

    #[cfg(feature = "sandbox")]
    {
        sequential_example().await?;
    }

    #[cfg(not(feature = "sandbox"))]
    {
        println!("This example requires the `sandbox` feature.");
        println!("Run with: cargo run --example sequential_sends --features sandbox");
    }

    Ok(())
}
