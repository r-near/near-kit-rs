use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::transaction::Final;
use near_kit::{AccountId, Near, NearToken};
use near_kit_sandbox::{SANDBOX_ROOT_ACCOUNT, Sandbox, SandboxConfig};
use tokio::sync::OnceCell;

static ACCOUNT_COUNTER: AtomicUsize = AtomicUsize::new(0);
static SANDBOX_2_13: OnceCell<Sandbox> = OnceCell::const_new();

pub(crate) fn unique_account(prefix: &str) -> AccountId {
    let n = ACCOUNT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{n}.{SANDBOX_ROOT_ACCOUNT}")
        .parse()
        .expect("generated sandbox account ID must be valid")
}

pub(crate) async fn shared_client() -> (&'static Sandbox, Near) {
    let sandbox = SandboxConfig::shared()
        .await
        .expect("shared sandbox must start");
    (sandbox, sandbox.client())
}

pub(crate) async fn sandbox_2_13() -> &'static Sandbox {
    SANDBOX_2_13
        .get_or_try_init(|| async { SandboxConfig::builder().version("2.13.4").fresh().await })
        .await
        .expect("2.13 sandbox must start")
}

pub(crate) fn client_for(
    sandbox: &Sandbox,
    account_id: &AccountId,
    secret_key: &SecretKey,
) -> Near {
    let signer = InMemorySigner::from_secret_key(account_id.clone(), secret_key.clone())
        .expect("typed sandbox account and key must create a signer");
    Near::sandbox(sandbox).with_signer(signer)
}

pub(crate) async fn funded_account(
    root: &Near,
    sandbox: &Sandbox,
    prefix: &str,
    balance: NearToken,
) -> (Near, AccountId, SecretKey) {
    let account_id = unique_account(prefix);
    let secret_key = SecretKey::generate_ed25519();

    root.transaction(&account_id)
        .create_account()
        .transfer(balance)
        .add_full_access_key(secret_key.public_key())
        .send()
        .wait_until::<Final>()
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
