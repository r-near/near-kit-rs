//! Fungible (NEP-141) and non-fungible (NEP-171) tokens, on a local sandbox.
//!
//! `near_kit::standards` has clients for both standards. Get one with
//! `contract::<FungibleToken>(id)` or `contract::<NonFungibleToken>(id)`: on a
//! read-only `Near` it can only view, on a `SigningNear` it can also send.
//!
//! What it shows:
//!
//! - Fungible tokens: `metadata()` (and its `decimals`), `storage_balance_bounds`
//!   and `storage_deposit` to register a receiver, `transfer`, `balance_of`.
//! - Non-fungible tokens: minting through a plain `FunctionCall` (minting isn't
//!   part of the standard), then `token`, `transfer` and `tokens_for_owner`.
//!
//! Token amounts are integers in the token's smallest unit; `decimals` says
//! where the point goes when you display one.
//!
//! Run it (needs Docker; starts a fresh sandbox and stops it on exit):
//!
//! ```text
//! cargo run -p near-kit-sandbox --example tokens --features integration-tests
//! ```

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::standards::{FungibleToken, NonFungibleToken};
use near_kit::transaction::FunctionCall;
use near_kit::{AccountId, Gas, NearToken, SigningNear, account};
use near_kit_sandbox::Sandbox;

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

/// Deploy `wasm` to `contract` and call its `new` initializer, in one
/// transaction.
async fn deploy(
    contract: &SigningNear,
    wasm: &[u8],
    init_args: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    contract
        .transaction(contract.account_id())
        .add_action(Action::deploy_contract(wasm.to_vec()))
        .add_action(
            FunctionCall::new("new")
                .args(init_args)
                .gas(Gas::from_tgas(50)),
        )
        .await?
        .result()?;
    Ok(())
}

/// `amount` in the token's smallest unit, written with `decimals` places.
fn display_amount(amount: u128, decimals: u8) -> String {
    let scale = 10u128.pow(decimals.into());
    let fraction = format!("{:0width$}", amount % scale, width = decimals.into());
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{}", amount / scale)
    } else {
        format!("{}.{fraction}", amount / scale)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().fresh().await?;
    let root = sandbox.root_account();
    let near = root.near();

    let alice = create_account(&root, account!("alice.sandbox"), NearToken::from_near(10)).await?;
    let bob = create_account(&root, account!("bob.sandbox"), NearToken::from_near(10)).await?;

    // --- Fungible token ---
    let ft_contract =
        create_account(&root, account!("ft.sandbox"), NearToken::from_near(10)).await?;
    deploy(
        &ft_contract,
        include_bytes!("../tests/contracts/fungible_token.wasm"),
        serde_json::json!({
            "owner_id": alice.account_id(),
            "total_supply": "1000000000", // 1,000.000000 with 6 decimals
            "metadata": { "spec": "ft-1.0.0", "name": "Example Token", "symbol": "EXT", "decimals": 6 },
        }),
    )
    .await?;

    // Alice's client sends as alice. Metadata is fetched once and cached.
    let token = alice.contract::<FungibleToken>(account!("ft.sandbox"));
    let metadata = token.metadata().await?;
    let decimals = metadata.decimals;
    println!(
        "{} ({}), {decimals} decimals",
        metadata.name, metadata.symbol
    );

    // A receiver must be registered (pay for its storage) before it can hold
    // the token. Anyone can register anyone; here alice pays for bob.
    if !token.is_registered(account!("bob.sandbox")).await? {
        let bounds = token.storage_balance_bounds().await?;
        token
            .storage_deposit(account!("bob.sandbox"), bounds.min)
            .await?
            .result()?;
        println!("registered bob for {}", bounds.min);
    }

    // 12.5 tokens: 12_500_000 in the smallest unit.
    token
        .transfer(
            account!("bob.sandbox"),
            12_500_000_u128,
            Some("lunch".into()),
        )
        .await?
        .result()?;

    // A read-only client is enough for balances.
    let token_view = near.contract::<FungibleToken>(account!("ft.sandbox"));
    let alice_balance = token_view.balance_of(alice.account_id()).await?;
    let bob_balance = token_view.balance_of(bob.account_id()).await?;
    println!(
        "alice: {} EXT, bob: {} EXT",
        display_amount(alice_balance, decimals),
        display_amount(bob_balance, decimals)
    );
    assert_eq!(bob_balance, 12_500_000);
    assert_eq!(
        alice_balance + bob_balance,
        token_view.total_supply().await?
    );

    // --- Non-fungible token ---
    let nft_contract =
        create_account(&root, account!("nft.sandbox"), NearToken::from_near(10)).await?;
    deploy(
        &nft_contract,
        include_bytes!("../tests/contracts/nft.wasm"),
        serde_json::json!({
            "owner_id": nft_contract.account_id(),
            "metadata": { "spec": "nft-1.0.0", "name": "Example Collection", "symbol": "EXC" },
        }),
    )
    .await?;

    // Minting is contract-specific, so it's a plain function call. This
    // contract lets its owner mint, with a deposit to cover storage.
    nft_contract
        .transaction(nft_contract.account_id())
        .add_action(
            FunctionCall::new("nft_mint")
                .args(serde_json::json!({
                    "token_id": "ticket-1",
                    "receiver_id": alice.account_id(),
                    "token_metadata": { "title": "Ticket #1" },
                }))
                .deposit(NearToken::from_millinear(10))
                .gas(Gas::from_tgas(50)),
        )
        .await?
        .result()?;

    let nft = alice.contract::<NonFungibleToken>(account!("nft.sandbox"));
    let ticket = nft
        .token("ticket-1")
        .await?
        .ok_or("ticket-1 was not minted")?;
    println!("ticket-1 belongs to {}", ticket.owner_id);

    // NEP-171 transfers attach 1 yoctoNEAR, which the client adds for you.
    nft.transfer(account!("bob.sandbox"), "ticket-1", None)
        .await?
        .result()?;

    let bobs = near
        .contract::<NonFungibleToken>(account!("nft.sandbox"))
        .tokens_for_owner(account!("bob.sandbox"), None, Some(10))
        .await?;
    let ids: Vec<&str> = bobs.iter().map(|token| token.token_id.as_str()).collect();
    println!("bob owns {ids:?}");
    assert_eq!(ids, ["ticket-1"]);

    println!("done");
    Ok(())
}
