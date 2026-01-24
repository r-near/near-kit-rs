//! Quickstart - Essential NEAR operations
//!
//! Covers: balance, view, call, transfer, multi-action transactions
//!
//! Run: cargo run --example quickstart
//!
//! Set environment variables for write operations:
//!   NEAR_ACCOUNT_ID=your-account.testnet
//!   NEAR_PRIVATE_KEY=ed25519:...

use near_kit::*;

// ============================================================================
// 1. View blockchain data (read-only, no credentials needed)
// ============================================================================

async fn view_example() -> Result<(), Error> {
    println!("=== View Example ===\n");

    let near = Near::testnet().build();

    // Check an account balance
    let balance = near.balance("alice.testnet").await?;
    println!("Alice's balance: {}", balance.available);

    // Call a view function on a contract
    let messages: Vec<serde_json::Value> = near
        .view("guestbook.near-examples.testnet", "get_messages")
        .args(serde_json::json!({ "from_index": "0", "limit": "5" }))
        .await?;

    println!("Guestbook has {} messages", messages.len());

    // Check if an account exists
    let exists = near.account_exists("alice.testnet").await?;
    println!("alice.testnet exists: {exists}");

    Ok(())
}

// ============================================================================
// 2. Call contract methods (requires credentials)
// ============================================================================

async fn call_example(near: &Near) -> Result<(), Error> {
    println!("\n=== Call Example ===\n");

    // Simple function call
    let outcome = near
        .call("guestbook.near-examples.testnet", "add_message")
        .args(serde_json::json!({ "text": "Hello from near-kit-rs!" }))
        .gas(Gas::tgas(30))
        .await?;

    println!("Transaction: {:?}", outcome.transaction_hash());

    Ok(())
}

// ============================================================================
// 3. Transfer NEAR tokens
// ============================================================================

async fn transfer_example(near: &Near) -> Result<(), Error> {
    println!("\n=== Transfer Example ===\n");

    // Transfer using typed amount
    let outcome = near
        .transfer("friend.testnet", NearToken::millinear(100))
        .await?;

    println!("Sent 0.1 NEAR: {:?}", outcome.transaction_hash());

    Ok(())
}

// ============================================================================
// 4. Multi-action transactions
// ============================================================================

async fn transaction_example(near: &Near, new_account: &str) -> Result<(), Error> {
    println!("\n=== Transaction Builder Example ===\n");

    // Generate a new keypair for the sub-account
    let keypair = KeyPair::random();

    println!("Creating sub-account: {new_account}");
    println!("With public key: {}", keypair.public_key);

    // Create account, fund it, and add a key - all in one atomic transaction
    let outcome = near
        .transaction(new_account)
        .create_account()
        .transfer(NearToken::near(1))
        .add_full_access_key(keypair.public_key)
        .send()
        .await?;

    println!("Transaction: {:?}", outcome.transaction_hash());

    Ok(())
}

// ============================================================================
// 5. Typed contract interface
// ============================================================================

#[near_kit::contract]
pub trait Guestbook {
    fn get_messages(&self, args: GetMessagesArgs) -> Vec<Message>;
    fn total_messages(&self) -> u32;

    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
}

// Note: The guestbook contract uses string types for pagination
#[derive(serde::Serialize)]
pub struct GetMessagesArgs {
    pub from_index: String,
    pub limit: String,
}

#[derive(serde::Serialize)]
pub struct AddMessageArgs {
    pub text: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Message {
    pub sender: String,
    pub text: String,
}

async fn typed_contract_example(near: &Near) -> Result<(), Error> {
    println!("\n=== Typed Contract Example ===\n");

    let guestbook = near.contract::<dyn Guestbook>("guestbook.near-examples.testnet");

    // View call with full type safety
    let total = guestbook.total_messages().await?;
    println!("Total messages: {total}");

    let messages = guestbook
        .get_messages(GetMessagesArgs {
            from_index: "0".to_string(),
            limit: "3".to_string(),
        })
        .await?;

    for msg in &messages {
        println!("  {} says: {}", msg.sender, msg.text);
    }

    // Change call with type safety
    guestbook
        .add_message(AddMessageArgs {
            text: "Type-safe message!".to_string(),
        })
        .gas(Gas::tgas(30))
        .await?;

    println!("Added message via typed interface");

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("near-kit-rs Quickstart Examples\n");

    // View examples work without credentials
    view_example().await?;

    // Check for credentials for write examples
    let account_id = std::env::var("NEAR_ACCOUNT_ID").ok();
    let private_key = std::env::var("NEAR_PRIVATE_KEY").ok();

    match (account_id, private_key) {
        (Some(account), Some(key)) => {
            let near = Near::testnet().credentials(&key, &account)?.build();

            call_example(&near).await?;
            transfer_example(&near).await?;

            // Create a unique sub-account name
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let new_account = format!("sub{timestamp}.{account}");
            transaction_example(&near, &new_account).await?;

            typed_contract_example(&near).await?;
        }
        _ => {
            println!("\n---");
            println!("Set NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY to run write examples.");
            println!("Get a testnet account at: https://testnet.mynearwallet.com/");
        }
    }

    Ok(())
}
