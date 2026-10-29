use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::{Final, FunctionCall};
use near_kit::{AccountId, Error, Gas, NearToken, SigningNear};
use near_kit_sandbox::{SANDBOX_ROOT_ACCOUNT, Sandbox};

static ACCOUNT_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn unique_account(prefix: &str) -> AccountId {
    let n = ACCOUNT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{n}.{SANDBOX_ROOT_ACCOUNT}")
        .parse()
        .expect("generated sandbox account ID must be valid")
}

pub(crate) async fn shared_client() -> (&'static Sandbox, SigningNear) {
    let sandbox = Sandbox::builder()
        .shared()
        .await
        .expect("shared sandbox must start");
    (sandbox, sandbox.root_account())
}

/// A fresh sandbox pinned to nearcore 2.13.4, owned by the calling test.
///
/// Not a shared `static`: statics are never dropped, so a shared fresh
/// sandbox would leave its container behind after every run. The library's
/// `.shared()` sandbox has its own exit-time cleanup; this one is removed when
/// the test drops it, including on panic.
pub(crate) async fn sandbox_2_13() -> Sandbox {
    Sandbox::builder()
        .version("2.13.4")
        .fresh()
        .await
        .expect("2.13 sandbox must start")
}

pub(crate) fn client_for(
    sandbox: &Sandbox,
    account_id: &AccountId,
    secret_key: &SecretKey,
) -> SigningNear {
    let signer = InMemorySigner::new(account_id.clone(), secret_key.clone());
    sandbox.root_account().near().with_signer(signer)
}

pub(crate) async fn funded_account(
    root: &SigningNear,
    sandbox: &Sandbox,
    prefix: &str,
    balance: NearToken,
) -> (SigningNear, AccountId, SecretKey) {
    let account_id = unique_account(prefix);
    let secret_key = SecretKey::generate_ed25519();

    root.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(balance))
        .add_action(Action::add_full_access_key(secret_key.public_key()))
        .await
        .expect("funded sandbox account must be created")
        .result()
        .unwrap();

    let client = client_for(sandbox, &account_id, &secret_key);
    (client, account_id, secret_key)
}

/// Deploy and initialize a fungible token whose whole supply (1M tokens, 18
/// decimals) belongs to `owner_id`; returns the contract account and its key.
pub(crate) async fn deploy_ft_contract(
    near: &SigningNear,
    owner_id: &AccountId,
) -> Result<(AccountId, SecretKey), Error> {
    let ft_key = SecretKey::generate_ed25519();
    let ft_id = unique_account("token");

    near.wait_until::<Final>()
        .transaction(&ft_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(50)))
        .add_action(Action::add_full_access_key(ft_key.public_key()))
        .add_action(Action::deploy_contract(fungible_token_wasm()))
        .await?
        .result()?;

    near.near()
        .with_signer(InMemorySigner::new(&ft_id, ft_key.clone()))
        .wait_until::<Final>()
        .transaction(&ft_id)
        .add_action(
            FunctionCall::new("new")
                .args(serde_json::json!({
                    "owner_id": owner_id.to_string(),
                    "total_supply": "1000000000000000000000000",
                    "metadata": {
                        "spec": "ft-1.0.0",
                        "name": "Test Token",
                        "symbol": "TEST",
                        "decimals": 18
                    }
                }))
                .gas(Gas::from_tgas(50)),
        )
        .await?
        .result()?;

    Ok((ft_id, ft_key))
}

pub(crate) fn guestbook_wasm() -> Vec<u8> {
    include_bytes!("../contracts/guestbook.wasm").to_vec()
}

pub(crate) fn fungible_token_wasm() -> Vec<u8> {
    include_bytes!("../contracts/fungible_token.wasm").to_vec()
}

pub(crate) fn nft_wasm() -> Vec<u8> {
    include_bytes!("../contracts/nft.wasm").to_vec()
}
