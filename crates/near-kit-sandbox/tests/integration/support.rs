use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::Final;
use near_kit::{AccountId, NearToken, SigningNear};
use near_kit_sandbox::{SANDBOX_ROOT_ACCOUNT, Sandbox};
use tokio::sync::OnceCell;

static ACCOUNT_COUNTER: AtomicUsize = AtomicUsize::new(0);
static SANDBOX_2_13: OnceCell<Sandbox> = OnceCell::const_new();

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
    (sandbox, sandbox.client())
}

pub(crate) async fn sandbox_2_13() -> &'static Sandbox {
    SANDBOX_2_13
        .get_or_try_init(|| async { Sandbox::builder().version("2.13.4").fresh().await })
        .await
        .expect("2.13 sandbox must start")
}

pub(crate) fn client_for(
    sandbox: &Sandbox,
    account_id: &AccountId,
    secret_key: &SecretKey,
) -> SigningNear {
    let signer = InMemorySigner::new(account_id.clone(), secret_key.clone());
    sandbox.client().near().with_signer(signer)
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
        .expect("funded sandbox account must be created");

    let client = client_for(sandbox, &account_id, &secret_key);
    (client, account_id, secret_key)
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
