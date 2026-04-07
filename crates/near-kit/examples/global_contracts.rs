//! Global Contracts: Publish and Deploy-From
//!
//! Demonstrates the global contract workflow:
//! 1. Publish a contract to the global registry
//! 2. Deploy it to another account using `deploy_from`
//! 3. Call methods on the deployed contract
//!
//! Run: cargo run --example global_contracts --features sandbox
//!
//! This example requires the `sandbox` feature and will start a local NEAR node.

use near_kit::*;

#[cfg(feature = "sandbox")]
use near_kit::sandbox::{Sandbox, SandboxConfig};

#[cfg(feature = "sandbox")]
async fn global_contracts_example() -> Result<(), Error> {
    println!("Starting local sandbox...\n");
    let sandbox: Sandbox = SandboxConfig::fresh().await;

    let root_near = sandbox.client();
    let root_account = root_near.account_id().to_string();

    // --- Create a publisher account ---
    let publisher_key = KeyPair::random();
    let publisher_account = format!("publisher.{root_account}");

    root_near
        .transaction(&publisher_account)
        .create_account()
        .transfer(NearToken::from_near(50))
        .add_full_access_key(publisher_key.public_key.clone())
        .send()
        .await?;

    let publisher_near = Near::custom(sandbox.rpc_url())
        .signer(InMemorySigner::from_secret_key(
            publisher_account.as_str(),
            publisher_key.secret_key,
        )?)
        .build();

    println!("Created publisher: {publisher_account}");

    // --- Publish a contract (updatable mode) ---
    let wasm_code = std::fs::read("tests/contracts/guestbook.wasm")
        .expect("Run from the crate root: cargo run --example global_contracts --features sandbox");

    publisher_near
        .publish(wasm_code.clone(), PublishMode::Updatable)
        .send()
        .wait_until(Final)
        .await?;

    println!("Published guestbook contract (updatable)\n");

    // --- Create a user account and deploy from the publisher ---
    let user_key = KeyPair::random();
    let user_account = format!("app.{root_account}");

    root_near
        .transaction(&user_account)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(user_key.public_key.clone())
        .send()
        .await?;

    let user_near = Near::custom(sandbox.rpc_url())
        .signer(InMemorySigner::from_secret_key(
            user_account.as_str(),
            user_key.secret_key,
        )?)
        .build();

    // Deploy from the publisher's global contract
    user_near
        .deploy_from(&publisher_account)
        .send()
        .wait_until(Final)
        .await?;

    println!("Deployed to {user_account} from publisher\n");

    // --- Interact with the deployed contract ---
    user_near
        .transaction(&user_account)
        .call("add_message")
        .args(serde_json::json!({ "text": "Hello from a global contract!" }))
        .gas(Gas::from_tgas(30))
        .deposit(NearToken::ZERO)
        .send()
        .wait_until(Final)
        .await?;

    let messages: Vec<serde_json::Value> = root_near
        .view(&user_account, "get_messages")
        .args(serde_json::json!({}))
        .await?;

    println!("Messages on {user_account}:");
    for msg in &messages {
        println!("  - {}", msg["text"]);
    }

    // --- Also demonstrate deploy_from with a CryptoHash ---
    let code_hash = CryptoHash::hash(&wasm_code);
    println!("\nContract code hash: {code_hash}");

    // Publish the same contract immutably (by hash)
    publisher_near
        .publish(wasm_code, PublishMode::Immutable)
        .send()
        .wait_until(Final)
        .await?;

    println!("Published same contract (immutable)\n");

    // Deploy to a second account using the hash
    let user2_key = KeyPair::random();
    let user2_account = format!("app2.{root_account}");

    root_near
        .transaction(&user2_account)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(user2_key.public_key.clone())
        .send()
        .await?;

    let user2_near = Near::custom(sandbox.rpc_url())
        .signer(InMemorySigner::from_secret_key(
            user2_account.as_str(),
            user2_key.secret_key,
        )?)
        .build();

    // Deploy from hash (type dispatches on CryptoHash)
    user2_near
        .deploy_from(code_hash)
        .send()
        .wait_until(Final)
        .await?;

    println!("Deployed to {user2_account} from code hash");

    // Verify it works
    let messages: Vec<serde_json::Value> = root_near
        .view(&user2_account, "get_messages")
        .args(serde_json::json!({}))
        .await?;

    println!(
        "Messages on {user2_account}: {} (empty as expected)",
        messages.len()
    );

    println!("\nDone.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Global Contracts Example\n");
    println!("Demonstrates publishing contracts to the global registry and deploying from them.\n");

    #[cfg(feature = "sandbox")]
    {
        global_contracts_example().await?;
    }

    #[cfg(not(feature = "sandbox"))]
    {
        println!("This example requires the `sandbox` feature.");
        println!("Run with: cargo run --example global_contracts --features sandbox");
    }

    Ok(())
}
