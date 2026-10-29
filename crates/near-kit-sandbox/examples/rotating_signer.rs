//! Concurrent transactions from one account with `RotatingSigner`.
//!
//! Every transaction carries a nonce that must be higher than the last one
//! used by its access key. Many concurrent sends through one key compete for
//! nonces; spreading them over several keys of the same account lets them go
//! out in parallel. `RotatingSigner` holds the keys and hands out the next one
//! for each transaction.
//!
//! What it shows:
//!
//! - Adding several full-access keys to an account in one transaction.
//! - `RotatingSigner::new(account, keys)` attached with `near.with_signer(..)`.
//! - Twenty transfers sent at once from cloned clients, all of which land.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example rotating_signer --features integration-tests
//! ```

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, RotatingSigner, SecretKey};
use near_kit::{AccountId, NearToken, SigningNear, account};
use near_kit_sandbox::Sandbox;

const KEYS: usize = 5;
const TRANSFERS: u128 = 20;

/// Create a sub-account of `root` with `balance` and a new key, and return a
/// client that signs as it.
async fn create_account(
    root: &SigningNear,
    account_id: AccountId,
    balance: NearToken,
) -> Result<SigningNear, Box<dyn std::error::Error>> {
    let key = SecretKey::generate_ed25519();
    root.transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(balance))
        .add_action(Action::add_full_access_key(key.public_key()))
        .await?
        // `?` on the send fails if the transaction didn't run, or if its
        // outcome is unknown (a timeout); `.result()?` also fails if an
        // action did.
        .result()?;
    Ok(root
        .near()
        .with_signer(InMemorySigner::new(account_id, key)))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();
    let near = root.near();

    create_account(&root, account!("alice.sandbox"), NearToken::from_near(1)).await?;

    // The bot account starts with one key and adds the rest itself.
    let keys: Vec<SecretKey> = (0..KEYS).map(|_| SecretKey::generate_ed25519()).collect();
    root.transaction(account!("bot.sandbox"))
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(keys[0].public_key()))
        .await?
        .result()?;
    let bot_first_key = near.with_signer(InMemorySigner::new(
        account!("bot.sandbox"),
        keys[0].clone(),
    ));
    keys[1..]
        .iter()
        .fold(
            bot_first_key.transaction(account!("bot.sandbox")),
            |tx, key| tx.add_action(Action::add_full_access_key(key.public_key())),
        )
        .await?
        .result()?;
    println!("bot.sandbox has {KEYS} keys");

    // One signing client over all five keys. Clones share the signer, so
    // each send through any clone takes the next key in turn.
    let bot = near.with_signer(RotatingSigner::new(account!("bot.sandbox"), keys)?);

    let before = near.balance(account!("alice.sandbox")).await?.total;
    let start = std::time::Instant::now();

    let sends = (0..TRANSFERS).map(|_| {
        let bot = bot.clone();
        async move {
            bot.transaction(account!("alice.sandbox"))
                .add_action(Action::transfer(NearToken::from_millinear(100)))
                .await
        }
    });
    let results = futures::future::join_all(sends).await;
    let elapsed = start.elapsed();

    let mut failed = 0;
    for result in &results {
        match result {
            Ok(outcome) if outcome.is_success() => {}
            Ok(outcome) => {
                failed += 1;
                println!(
                    "action failed: {}",
                    outcome.failure_message().unwrap_or_default()
                );
            }
            Err(error) => {
                failed += 1;
                println!("send failed: {error}");
            }
        }
    }
    println!(
        "{} of {TRANSFERS} transfers landed in {elapsed:.2?} ({:.1} tx/s)",
        results.len() - failed,
        results.len() as f64 / elapsed.as_secs_f64()
    );

    let received = near
        .balance(account!("alice.sandbox"))
        .await?
        .total
        .saturating_sub(before);
    println!("alice received {received}");
    if failed > 0 || received != NearToken::from_millinear(100 * TRANSFERS) {
        return Err(format!("{failed} transfers failed").into());
    }

    println!("done");
    Ok(())
}
