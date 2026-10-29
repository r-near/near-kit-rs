//! A helper the sandbox examples share.

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::{AccountId, NearToken, SigningNear};

/// Create a sub-account of `root` with `balance` and a new key, and return a
/// client that signs as it.
pub async fn create_account(
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
