//! Ordered queues, one per access key, with `into_per_key_signers`.
//!
//! When the order of an account's transactions matters, send them one at a
//! time per key: a transaction sent before the previous one is in a block can
//! reach a validator first and be rejected for its nonce. To keep throughput,
//! give the account several keys and run one sequential queue per key.
//!
//! What it shows:
//!
//! - `RotatingSigner::into_per_key_signers()`: one `InMemorySigner` per key.
//! - A task per key that sends its transactions in order, each waiting for
//!   `Included` (in a block) before the next, through
//!   `wait_until::<Included>()`.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example sequential_sends --features integration-tests
//! ```

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, RotatingSigner, SecretKey};
use near_kit::transaction::Included;
use near_kit::{AccountId, NearToken, SigningNear, account};
use near_kit_sandbox::Sandbox;

const KEYS: usize = 3;
const PER_KEY: u128 = 5;

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
        // `?` on the send fails only if the transaction didn't execute;
        // `.result()?` also fails if an action did.
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

    let before = near.balance(account!("alice.sandbox")).await?.total;
    let start = std::time::Instant::now();

    // One task per key. Each `bot` signs with a single key and waits only
    // until its transaction is in a block, the earliest point at which the
    // next nonce from that key is safe to send.
    let per_key = RotatingSigner::new(account!("bot.sandbox"), keys)?.into_per_key_signers();
    let queues: Vec<_> = per_key
        .into_iter()
        .enumerate()
        .map(|(queue, signer)| {
            let bot = near.with_signer(signer).wait_until::<Included>();
            tokio::spawn(async move {
                for n in 0..PER_KEY {
                    let response = bot
                        .transaction(account!("alice.sandbox"))
                        .add_action(Action::transfer(NearToken::from_millinear(10)))
                        .await?;
                    println!(
                        "key {queue}: transfer {n} included ({})",
                        response.transaction_hash
                    );
                }
                Ok::<_, near_kit::Error>(())
            })
        })
        .collect();

    for queue in queues {
        queue.await??;
    }
    println!(
        "{} transfers in {:.2?}",
        KEYS as u128 * PER_KEY,
        start.elapsed()
    );

    // `Included` returns before execution, so the balance may still be
    // catching up. Read at `Final` and poll briefly.
    let expected = NearToken::from_millinear(10 * KEYS as u128 * PER_KEY);
    for _ in 0..20 {
        let received = near
            .balance(account!("alice.sandbox"))
            .await?
            .total
            .saturating_sub(before);
        if received == expected {
            println!("alice received {received}");
            println!("done");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(format!("alice did not receive {expected}").into())
}
