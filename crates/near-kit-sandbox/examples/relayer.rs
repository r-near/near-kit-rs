//! A relayer for meta-transactions (NEP-366), on a local sandbox.
//!
//! A user signs a delegate action and hands it to a relayer as base64. The
//! relayer decodes it, checks it against its policy, wraps it in a transaction
//! of its own and pays for it. The contract sees the user as the caller.
//!
//! What it shows:
//!
//! - User side: `SigningNear::sign_delegate` with `DelegateOptions`, and
//!   `SignedDelegateAction::to_base64` as the payload to send over HTTP.
//! - Relayer side: `SignedDelegateAction::from_base64`, then the checks a
//!   relayer makes before paying: the signature, the sender, the receiver, which
//!   actions are allowed, gas, deposits and expiry. Then it sends
//!   `Action::delegate(signed)` in a transaction to `signed.sender_id()` (the
//!   user's account, not the contract).
//! - Reading the result: the relayer's transaction succeeds even when the
//!   user's call fails, so the relayer looks at the receipt outcomes.
//!
//! The relayer pays for two things: the gas, and any deposit attached to the
//! user's actions. The deposit comes out of the relayer's balance, and goes to
//! the user if the call fails. That is why this policy allows no deposits.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example relayer --features integration-tests
//! ```

use std::fmt;

use near_kit::protocol::{Action, ActionError, SignedDelegateAction};
use near_kit::rpc::{FinalExecutionOutcome, Finality};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::{DelegateOptions, FunctionCall};
use near_kit::{AccountId, Error, Gas, NearToken, SigningNear, account};
use near_kit_sandbox::Sandbox;

/// What this relayer agrees to pay for.
struct Policy {
    /// Accounts the relayer serves (its signed-up users).
    users: Vec<AccountId>,
    /// The one contract it pays calls to.
    receiver: AccountId,
    /// Methods on that contract it pays for.
    methods: &'static [&'static str],
    /// Gas cap across all of a request's function calls.
    max_gas: Gas,
    /// A request must stay valid at least this many blocks from now, so it
    /// doesn't expire before the relayer's transaction lands.
    min_blocks_left: u64,
    /// ...and expire within this many, so an old signature can't be
    /// replayed into the relayer much later.
    max_blocks_left: u64,
}

/// Why a relay request did not go through.
#[derive(Debug)]
enum RelayError {
    /// The request broke the policy. Nothing was sent and nothing was paid.
    Rejected(String),
    /// The transaction did not execute (RPC error, invalid transaction).
    Near(Error),
    /// The transaction executed but the user's action failed. The relayer
    /// still paid for the gas.
    UserActionFailed(Box<ActionError>),
}

impl fmt::Display for RelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(reason) => write!(f, "rejected: {reason}"),
            Self::Near(error) => write!(f, "not executed: {error}"),
            Self::UserActionFailed(error) => write!(f, "user action failed: {error}"),
        }
    }
}

impl std::error::Error for RelayError {}

impl From<Error> for RelayError {
    fn from(error: Error) -> Self {
        Self::Near(error)
    }
}

/// Check a decoded request against the policy at block height `height`.
/// Runs before anything is sent.
fn check(policy: &Policy, signed: &SignedDelegateAction, height: u64) -> Result<(), String> {
    let delegate = &signed.delegate_action;

    // The signature covers every field below, so check it first: a request
    // edited after signing would otherwise pass the policy and fail on chain,
    // at the relayer's expense.
    if !signed.verify() {
        return Err("signature doesn't match the delegate action".into());
    }
    // The chain checks that the key belongs to the sender and may call this
    // receiver; a relayer can check it up front with
    // `near.rpc().view_access_key(..)`.

    if !policy.users.contains(&delegate.sender_id) {
        return Err(format!(
            "{} is not a user of this relayer",
            delegate.sender_id
        ));
    }
    if delegate.receiver_id != policy.receiver {
        return Err(format!("receiver {} is not allowed", delegate.receiver_id));
    }

    let mut gas = Gas::from_gas(0);
    for action in &delegate.actions {
        let Action::FunctionCall(call) = action.inner() else {
            return Err(format!(
                "only function calls are allowed, got {:?}",
                action.inner()
            ));
        };
        if !policy.methods.contains(&call.method_name.as_str()) {
            return Err(format!("method {} is not allowed", call.method_name));
        }
        if !call.deposit.is_zero() {
            return Err(format!("deposits are not allowed, got {}", call.deposit));
        }
        gas = gas.saturating_add(call.gas);
    }
    if gas > policy.max_gas {
        return Err(format!("{gas} is over the {} limit", policy.max_gas));
    }

    let blocks_left = delegate.max_block_height.saturating_sub(height);
    if blocks_left < policy.min_blocks_left {
        return Err(format!(
            "expires at block {}, current block is {height}",
            delegate.max_block_height
        ));
    }
    if blocks_left > policy.max_blocks_left {
        return Err(format!(
            "valid for {blocks_left} blocks, the limit is {}",
            policy.max_blocks_left
        ));
    }

    Ok(())
}

/// Handle one relay request: the body a user POSTs to the relayer.
async fn relay(
    relayer: &SigningNear,
    policy: &Policy,
    payload: &str,
) -> Result<FinalExecutionOutcome, RelayError> {
    let signed = SignedDelegateAction::from_base64(payload)
        .map_err(|error| RelayError::Rejected(format!("not a signed delegate action: {error}")))?;

    let height = relayer
        .near()
        .rpc()
        .block(Finality::Final)
        .await
        .map_err(Error::from)?
        .header
        .height;
    check(policy, &signed, height).map_err(RelayError::Rejected)?;

    // The transaction goes to the delegate's sender: the user's account
    // unpacks the delegate action and forwards its actions to the receiver.
    // near-kit rejects any other receiver before signing (`Error::Validation`).
    let outcome = relayer
        .transaction(signed.sender_id())
        .add_action(Action::delegate(signed))
        .await?;

    // The final status follows the receipt that unpacks the delegate action on
    // the user's account, which succeeds once it has forwarded the actions.
    // The user's call runs in a later receipt, so `outcome.is_failure()`
    // doesn't see it: look through the receipt outcomes.
    if let Some(error) = outcome
        .receipts_outcome
        .iter()
        .find_map(|receipt| receipt.outcome.status.failure())
    {
        return Err(RelayError::UserActionFailed(Box::new(error.clone())));
    }
    Ok(outcome)
}

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

/// A signed `add_message` request from `user`, as the base64 a relayer receives.
async fn add_message_request(
    user: &SigningNear,
    receiver: AccountId,
    call: FunctionCall,
) -> Result<String, Error> {
    let signed = user
        .sign_delegate(
            receiver,
            vec![call.try_into()?],
            // Valid for 100 blocks from now (the default is 200).
            DelegateOptions {
                block_height_offset: Some(100),
                ..Default::default()
            },
        )
        .await?;
    Ok(signed.to_base64())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();
    let near = root.near();

    let guestbook = create_account(
        &root,
        account!("guestbook.sandbox"),
        NearToken::from_near(10),
    )
    .await?;
    guestbook
        .transaction(guestbook.account_id())
        .add_action(Action::deploy_contract(
            include_bytes!("../tests/contracts/guestbook.wasm").to_vec(),
        ))
        .await?
        .result()?;

    // The user needs an account and a key, but spends nothing.
    let alice = create_account(&root, account!("alice.sandbox"), NearToken::from_near(1)).await?;
    // Sender policy is per client: retry nonce clashes harder than the default 3.
    let relayer = create_account(&root, account!("relayer.sandbox"), NearToken::from_near(10))
        .await?
        .max_nonce_retries(10);

    let policy = Policy {
        users: vec![alice.account_id().clone()],
        receiver: guestbook.account_id().clone(),
        methods: &["add_message"],
        max_gas: Gas::from_tgas(50),
        min_blocks_left: 5,
        max_blocks_left: 300,
    };

    let alice_before = near.balance(alice.account_id()).await?.total;
    let relayer_before = near.balance(relayer.account_id()).await?.total;

    // --- A request the relayer accepts ---
    let payload = add_message_request(
        &alice,
        account!("guestbook.sandbox"),
        FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": "Sent through a relayer" }))
            .gas(Gas::from_tgas(30)),
    )
    .await?;
    println!("payload: {} bytes of base64", payload.len());

    let outcome = relay(&relayer, &policy, &payload).await?;
    println!("relayed: {}", outcome.transaction_hash());

    let messages: Vec<serde_json::Value> = near
        .view(account!("guestbook.sandbox"), "get_messages")
        .args(serde_json::json!({}))
        .await?;
    println!("message sender: {}", messages[0]["sender"]);
    assert_eq!(messages[0]["sender"], "alice.sandbox");

    let alice_after = near.balance(alice.account_id()).await?.total;
    let relayer_after = near.balance(relayer.account_id()).await?.total;
    println!(
        "alice paid {}, relayer paid {}",
        alice_before.saturating_sub(alice_after),
        relayer_before.saturating_sub(relayer_after)
    );
    assert_eq!(alice_before, alice_after);

    // --- Requests the relayer turns down before sending anything ---
    let with_deposit = add_message_request(
        &alice,
        account!("guestbook.sandbox"),
        FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": "premium, on the relayer" }))
            .gas(Gas::from_tgas(30))
            .deposit(NearToken::from_near(1)),
    )
    .await?;
    let other_method = add_message_request(
        &alice,
        account!("guestbook.sandbox"),
        FunctionCall::new("new").gas(Gas::from_tgas(30)),
    )
    .await?;
    let too_much_gas = add_message_request(
        &alice,
        account!("guestbook.sandbox"),
        FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": "hi" }))
            .gas(Gas::from_tgas(200)),
    )
    .await?;
    let other_receiver = add_message_request(
        &alice,
        account!("relayer.sandbox"),
        FunctionCall::new("add_message").gas(Gas::from_tgas(30)),
    )
    .await?;
    // Valid for 1000 blocks, over the policy's 300.
    let long_lived = {
        let call = FunctionCall::new("add_message")
            .args(serde_json::json!({ "text": "hi" }))
            .gas(Gas::from_tgas(30));
        let options = DelegateOptions {
            block_height_offset: Some(1000),
            ..Default::default()
        };
        alice
            .sign_delegate(
                account!("guestbook.sandbox"),
                vec![call.try_into()?],
                options,
            )
            .await?
            .to_base64()
    };
    // Edited after signing: the signature no longer matches.
    let tampered = {
        let mut signed = SignedDelegateAction::from_base64(&payload)?;
        signed.delegate_action.max_block_height += 1;
        signed.to_base64()
    };
    let not_a_payload = "aGVsbG8=".to_string();

    for (name, request) in [
        ("deposit", with_deposit),
        ("method", other_method),
        ("gas", too_much_gas),
        ("receiver", other_receiver),
        ("lifetime", long_lived),
        ("signature", tampered),
        ("garbage", not_a_payload),
    ] {
        match relay(&relayer, &policy, &request).await {
            Err(error @ RelayError::Rejected(_)) => println!("{name}: {error}"),
            other => return Err(format!("{name}: expected a rejection, got {other:?}").into()),
        }
    }

    // --- A request that passes the policy but fails on chain ---
    // `add_message` without its `text` argument panics in the contract. The
    // relayer's transaction still succeeds and the gas is spent.
    let bad_args = add_message_request(
        &alice,
        account!("guestbook.sandbox"),
        FunctionCall::new("add_message")
            .args(serde_json::json!({}))
            .gas(Gas::from_tgas(30)),
    )
    .await?;
    match relay(&relayer, &policy, &bad_args).await {
        Err(error @ RelayError::UserActionFailed(_)) => println!("bad args: {error}"),
        other => return Err(format!("bad args: expected a failed action, got {other:?}").into()),
    }

    println!("done");
    Ok(())
}
