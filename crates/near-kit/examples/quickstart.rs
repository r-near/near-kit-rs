//! Quickstart: read from testnet, then write if you give it an account.
//!
//! What it shows:
//!
//! - A read-only `Near` client: `balance`, `account_exists`, and a contract
//!   view with `view(..).args(..)`, read at `Final` (the default) and at
//!   `Optimistic` with `.at(..)`.
//! - A `SigningNear` from `SigningNear::from_env()`, and one transaction with
//!   several actions: it creates a sub-account of your account, funds it and
//!   gives it a key; a second transaction deletes it and returns what's left
//!   to you. Nothing is sent to any other account.
//! - A failed action is `Ok(outcome)` with `outcome.is_failure()`; `Err`
//!   means the transaction didn't run, or (after a timeout) that near-kit
//!   couldn't tell whether it did.
//!
//! Reads only (no account needed):
//!
//! ```text
//! cargo run -p near-kit --example quickstart
//! ```
//!
//! With writes, as your testnet account (costs a little gas):
//!
//! ```text
//! NEAR_ACCOUNT_ID=<you>.testnet NEAR_PRIVATE_KEY=ed25519:... \
//!     cargo run -p near-kit --example quickstart
//! ```
//!
//! To get a testnet account, near-cli-rs can create one paid for by the
//! faucet (`near account create-account sponsor-by-faucet-service
//! <you>.testnet autogenerate-new-keypair save-to-keychain network-config
//! testnet create`), and print its private key (`near account export-account
//! <you>.testnet using-private-key network-config testnet`).
//!
//! `Near::testnet()` uses FastNEAR's free, rate-limited RPC endpoint. Set
//! `NEAR_NETWORK` to `mainnet`, or to an RPC URL together with `NEAR_CHAIN_ID`
//! (required with a URL), to point the writes elsewhere.

use near_kit::protocol::Action;
use near_kit::rpc::Finality;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::{AccountId, Error, Near, NearToken, SigningNear, account};

async fn reads(near: &Near) -> Result<(), Error> {
    // Balances come back as `NearToken`s; `available` excludes what's
    // locked for storage.
    let balance = near
        .balance(account!("guestbook.near-examples.testnet"))
        .await?;
    println!(
        "guestbook balance: {} ({} available)",
        balance.total, balance.available
    );

    let exists = near
        .account_exists(account!("no-such-account.testnet"))
        .await?;
    println!("no-such-account.testnet exists: {exists}");

    // A contract view: JSON arguments in, a JSON result decoded into `T`.
    let total: u64 = near
        .view(
            account!("guestbook.near-examples.testnet"),
            "total_messages",
        )
        .await?;
    let latest: Vec<serde_json::Value> = near
        .view(account!("guestbook.near-examples.testnet"), "get_messages")
        .args(
            serde_json::json!({ "from_index": total.saturating_sub(3).to_string(), "limit": "3" }),
        )
        // Reads default to `Final`; `Optimistic` is newer but can still change.
        .at(Finality::Optimistic)
        .await?;
    println!("guestbook has {total} messages; the latest:");
    for message in &latest {
        println!("  {}: {}", message["sender"], message["text"]);
    }
    Ok(())
}

async fn writes(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
    // A fresh name for each run.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let sub_id: AccountId = format!("quickstart-{seconds}.{}", alice.account_id()).parse()?;
    let sub_key = SecretKey::generate_ed25519();

    // Several actions, one transaction: they all apply or none do.
    let outcome = alice
        .transaction(&sub_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_millinear(10)))
        .add_action(Action::add_full_access_key(sub_key.public_key()))
        .await?;
    // Past the `?`, the transaction ran, but an action can still have
    // failed (an existing account, too little balance): check.
    if outcome.is_failure() {
        return Err(format!(
            "creating {sub_id} failed: {}",
            outcome.failure_message().unwrap_or_default()
        )
        .into());
    }
    println!("created {sub_id} in {}", outcome.transaction_hash());

    // Sign as the new account over the same connection, and delete it,
    // sending its balance back. `.result()?` turns a failed action into an
    // error.
    let sub = alice
        .near()
        .with_signer(InMemorySigner::new(sub_id.clone(), sub_key));
    let outcome = sub
        .transaction(&sub_id)
        .add_action(Action::delete_account(alice.account_id().clone()))
        .await?;
    outcome.result()?;
    println!("deleted {sub_id} in {}", outcome.transaction_hash());

    // Sends wait for `Final` by default, so this read already sees both.
    let balance = alice.near().balance(alice.account_id()).await?;
    println!("{} now has {}", alice.account_id(), balance.total);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let near = Near::testnet().build();
    reads(&near).await?;

    // `from_env` reads NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY, and optionally
    // NEAR_NETWORK and NEAR_CHAIN_ID (required when NEAR_NETWORK is a URL).
    match SigningNear::from_env() {
        Ok(alice) => writes(&alice).await?,
        Err(error) => println!("\nskipping writes: {error}"),
    }
    Ok(())
}
