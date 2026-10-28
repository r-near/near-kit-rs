//! Global contracts: publish code once, deploy it to many accounts.
//!
//! A global contract is code stored on chain under a publisher account or its
//! code hash. Other accounts then use it with `Action::deploy_from` instead of
//! uploading the wasm themselves.
//!
//! What it shows:
//!
//! - `Action::publish(code, PublishMode::Updatable)`: identified by the
//!   publisher's account, which can publish new code under it later.
//! - `Action::publish(code, PublishMode::Immutable)`: identified by the code
//!   hash, which never changes.
//! - `Action::deploy_from(account_id)` and `Action::deploy_from(code_hash)`,
//!   then a call and a view on each deployed account.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example global_contracts --features integration-tests
//! ```

use near_kit::protocol::{Action, PublishMode};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::FunctionCall;
use near_kit::{AccountId, CryptoHash, Gas, NearToken, SigningNear, account};
use near_kit_sandbox::Sandbox;

const GUESTBOOK_WASM: &[u8] = include_bytes!("../tests/contracts/guestbook.wasm");

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

/// Add a message to the guestbook deployed on `app`, then read them back.
async fn try_guestbook(app: &SigningNear) -> Result<usize, Box<dyn std::error::Error>> {
    app.transaction(app.account_id())
        .add_action(
            FunctionCall::new("add_message")
                .args(serde_json::json!({ "text": "Hello from a global contract" }))
                .gas(Gas::from_tgas(30)),
        )
        .await?
        .result()?;

    let messages: Vec<serde_json::Value> = app
        .near()
        .view(app.account_id(), "get_messages")
        .args(serde_json::json!({}))
        .await?;
    Ok(messages.len())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();

    // Publishing pays for the code's storage, so the publisher needs a balance.
    let publisher = create_account(
        &root,
        account!("publisher.sandbox"),
        NearToken::from_near(50),
    )
    .await?;

    // --- By publisher account (updatable) ---
    publisher
        .transaction(publisher.account_id())
        .add_action(Action::publish(GUESTBOOK_WASM, PublishMode::Updatable))
        .await?
        .result()?;
    println!("published the guestbook under {}", publisher.account_id());

    let app = create_account(&root, account!("app.sandbox"), NearToken::from_near(10)).await?;
    app.transaction(app.account_id())
        .add_action(Action::deploy_from(publisher.account_id().clone()))
        .await?
        .result()?;
    println!(
        "deployed to {} from {}",
        app.account_id(),
        publisher.account_id()
    );

    let count = try_guestbook(&app).await?;
    println!("{} has {count} message", app.account_id());
    assert_eq!(count, 1);

    // --- By code hash (immutable) ---
    let code_hash = CryptoHash::hash(GUESTBOOK_WASM);
    publisher
        .transaction(publisher.account_id())
        .add_action(Action::publish(GUESTBOOK_WASM, PublishMode::Immutable))
        .await?
        .result()?;
    println!("published the guestbook as {code_hash}");

    // `deploy_from` takes an account id or a code hash.
    let app2 = create_account(&root, account!("app2.sandbox"), NearToken::from_near(10)).await?;
    app2.transaction(app2.account_id())
        .add_action(Action::deploy_from(code_hash))
        .await?
        .result()?;
    println!("deployed to {} from {code_hash}", app2.account_id());

    // Each account has its own state; only the code is shared.
    let count = try_guestbook(&app2).await?;
    println!("{} has {count} message", app2.account_id());
    assert_eq!(count, 1);

    println!("done");
    Ok(())
}
