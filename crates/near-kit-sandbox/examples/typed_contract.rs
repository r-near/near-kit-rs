//! A typed contract client with `#[near_kit::contract]`, on a local sandbox.
//!
//! Declare the contract's methods as a Rust trait and the macro generates a
//! client: views return the decoded value, calls return a builder you can set
//! gas and deposit on before awaiting. The same macro gives an offline
//! `FunctionCall` constructor for each call, to batch into a transaction.
//!
//! What it shows, against the guestbook contract:
//!
//! - Views through a read-only `Near`, at the default `Final` finality and at
//!   an earlier block with `.at(..)`.
//! - Calls through a `SigningNear`, with `.gas(..)` and `.deposit(..)`.
//! - `Guestbook::add_message(text)` as an action in a `Tx`, batched with
//!   another one.
//! - A failed action is `Ok(outcome)` with `outcome.is_failure()`, and the
//!   actions in a transaction succeed or fail together.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example typed_contract --features integration-tests
//! ```

mod common;

use common::create_account;
use near_kit::protocol::Action;
use near_kit::rpc::Finality;
use near_kit::transaction::FunctionCall;
use near_kit::{AccountId, Gas, NearToken, account};
use near_kit_sandbox::Sandbox;
use serde::Deserialize;

/// The guestbook contract's interface. Methods taking `&self` are views;
/// `#[call]` methods taking `&mut self` are transactions. Each parameter is
/// sent by name, as the contract declares it (`{"text": ".."}`).
#[near_kit::contract]
pub trait Guestbook {
    fn total_messages(&self) -> u32;

    /// The contract takes `U128`/`U64` numbers, which are strings in JSON.
    fn get_messages(&self, from_index: &str, limit: &str) -> Vec<Message>;

    #[call]
    fn add_message(&mut self, text: &str);
}

#[derive(Debug, Deserialize)]
pub struct Message {
    /// Set when the call attached a deposit (0.1 NEAR does it).
    pub premium: bool,
    pub sender: AccountId,
    pub text: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();
    let near = root.near();

    let contract = create_account(
        &root,
        account!("guestbook.sandbox"),
        NearToken::from_near(10),
    )
    .await?;
    contract
        .transaction(contract.account_id())
        .add_action(Action::deploy_contract(
            include_bytes!("../tests/contracts/guestbook.wasm").to_vec(),
        ))
        .await?
        .result()?;
    let alice = create_account(&root, account!("alice.sandbox"), NearToken::from_near(10)).await?;

    // --- Calls, through a client that signs as alice ---
    let guestbook = alice.contract::<Guestbook>(account!("guestbook.sandbox"));

    let outcome = guestbook
        .add_message("Hello from a typed client")
        .gas(Gas::from_tgas(10))
        .await?;
    println!("add_message: {}", outcome.transaction_hash());
    // SigningNear waits for `Final` by default, so a default read sees it.
    let height_after_first = near.rpc().block(Finality::Final).await?.header.height;

    // Attaching 0.1 NEAR makes this guestbook mark the message premium.
    guestbook
        .add_message("A premium message")
        .deposit(NearToken::from_millinear(100))
        .await?
        .result()?;

    // --- Views, through the read-only client ---
    let guestbook_view = near.contract::<Guestbook>(account!("guestbook.sandbox"));

    let total = guestbook_view.total_messages().await?;
    let earlier = guestbook_view
        .total_messages()
        .at(height_after_first)
        .await?;
    let optimistic = guestbook_view
        .total_messages()
        .at(Finality::Optimistic)
        .await?;
    println!(
        "messages: {total} now, {earlier} at block {height_after_first}, {optimistic} optimistic"
    );
    assert_eq!((total, earlier), (2, 1));

    let messages = guestbook_view.get_messages("0", "10").await?;
    for message in &messages {
        println!(
            "  {} (premium: {}): {}",
            message.sender, message.premium, message.text
        );
    }
    assert!(messages[1].premium);

    // --- Batching: typed calls as actions in one transaction ---
    let outcome = alice
        .transaction(account!("guestbook.sandbox"))
        .add_action(Guestbook::add_message("First of two"))
        .add_action(Guestbook::add_message("Second of two"))
        .await?;
    println!("batch succeeded: {}", outcome.is_success());
    assert_eq!(guestbook_view.total_messages().await?, 4);

    // --- A failed action is Ok(outcome), and reverts the whole batch ---
    // The second action calls a method the contract doesn't have. The send
    // still returns Ok: the transaction executed. Check the outcome.
    let outcome = alice
        .transaction(account!("guestbook.sandbox"))
        .add_action(Guestbook::add_message("Never stored"))
        .add_action(FunctionCall::new("no_such_method").gas(Gas::from_tgas(10)))
        .await?;
    assert!(outcome.is_failure());
    println!(
        "batch failed: {}",
        outcome.failure_message().unwrap_or_default()
    );
    // The first action's message was rolled back with it.
    assert_eq!(guestbook_view.total_messages().await?, 4);

    println!("done");
    Ok(())
}
