//! Global Contracts: Publish and Deploy-From
//!
//! Demonstrates the global contract workflow:
//! 1. Publish a contract to the global registry
//! 2. Deploy it to another account using `deploy_from`
//! 3. Call methods on the deployed contract
//!
//! Run: cargo run -p near-kit-sandbox --example global_contracts --features integration-tests
//!
//! This example starts a local NEAR node through Docker.

use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_kit::*;

use near_kit::protocol::PublishMode;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::Final;

use near_kit_sandbox::{Sandbox, SandboxConfig};

async fn global_contracts_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting local sandbox...\n");
    let sandbox: Sandbox = SandboxConfig::fresh().await?;

    let root_near = sandbox.client();
    let root_account = root_near.account_id().to_string();

    // --- Create a publisher account ---
    let publisher_key = SecretKey::generate_ed25519();
    let publisher_account: AccountId = format!("publisher.{root_account}").parse()?;

    root_near
        .transaction(&publisher_account)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(publisher_key.public_key()))
        .await?;

    let publisher_near = sandbox
        .client()
        .near()
        .with_signer(InMemorySigner::from_secret_key(
            &publisher_account,
            publisher_key,
        )?);

    println!("Created publisher: {publisher_account}");

    // --- Publish a contract (updatable mode) ---
    let wasm_code = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/contracts/guestbook.wasm"
    ))
    .to_vec();

    publisher_near
        .wait_until::<Final>()
        .transaction(publisher_near.account_id())
        .add_action(Action::publish(wasm_code.clone(), PublishMode::Updatable))
        .await?;

    println!("Published guestbook contract (updatable)\n");

    // --- Create a user account and deploy from the publisher ---
    let user_key = SecretKey::generate_ed25519();
    let user_account: AccountId = format!("app.{root_account}").parse()?;

    root_near
        .transaction(&user_account)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(user_key.public_key()))
        .await?;

    let user_near = sandbox
        .client()
        .near()
        .with_signer(InMemorySigner::from_secret_key(&user_account, user_key)?);

    // Deploy from the publisher's global contract
    user_near
        .wait_until::<Final>()
        .transaction(user_near.account_id())
        .add_action(Action::deploy_from_account(publisher_account.clone()))
        .await?;

    println!("Deployed to {user_account} from publisher\n");

    // --- Interact with the deployed contract ---
    user_near
        .wait_until::<Final>()
        .transaction(&user_account)
        .add_action(
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "Hello from a global contract!" }))
                .gas(Gas::from_tgas(30))
                .deposit(NearToken::ZERO),
        )
        .await?;

    let messages: Vec<serde_json::Value> = root_near
        .near()
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
        .wait_until::<Final>()
        .transaction(publisher_near.account_id())
        .add_action(Action::publish(wasm_code, PublishMode::Immutable))
        .await?;

    println!("Published same contract (immutable)\n");

    // Deploy to a second account using the hash
    let user2_key = SecretKey::generate_ed25519();
    let user2_account: AccountId = format!("app2.{root_account}").parse()?;

    root_near
        .transaction(&user2_account)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(user2_key.public_key()))
        .await?;

    let user2_near = sandbox
        .client()
        .near()
        .with_signer(InMemorySigner::from_secret_key(&user2_account, user2_key)?);

    // Deploy from hash (type dispatches on CryptoHash)
    user2_near
        .wait_until::<Final>()
        .transaction(user2_near.account_id())
        .add_action(Action::deploy_from_hash(code_hash))
        .await?;

    println!("Deployed to {user2_account} from code hash");

    // Verify it works
    let messages: Vec<serde_json::Value> = root_near
        .near()
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Global Contracts Example\n");
    println!("Demonstrates publishing contracts to the global registry and deploying from them.\n");

    global_contracts_example().await?;

    Ok(())
}
