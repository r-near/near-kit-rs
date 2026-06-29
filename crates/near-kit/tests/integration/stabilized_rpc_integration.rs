//! Integration tests for the stabilized 2.13 RPC method wrappers:
//! `block_effects`, `genesis_config`, and `maintenance_windows`.

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("blockfx{n}.{SANDBOX_ROOT_ACCOUNT}")
        .parse()
        .unwrap()
}

#[tokio::test]
async fn test_genesis_config_returns_chain_config() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let config = near.rpc().genesis_config().await.expect("genesis_config");
    // The genesis document always carries a chain id and a protocol version.
    assert!(
        config.get("chain_id").and_then(|v| v.as_str()).is_some(),
        "genesis_config should include chain_id, got: {config}"
    );
    assert!(
        config
            .get("protocol_version")
            .and_then(|v| v.as_u64())
            .is_some(),
        "genesis_config should include a numeric protocol_version, got: {config}"
    );
}

#[tokio::test]
async fn test_block_effects_returns_changes_for_block() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // Produce a block we know contains state changes: create an account. Then
    // query that exact block's effects by hash (a fixed block, not the moving
    // `final` reference) and assert the kind-changes parse — this exercises the
    // non-empty path that an idle `final` block would not.
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();
    let outcome = near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(3))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(Final)
        .await
        .expect("create_account must execute");

    let block_hash = outcome.transaction_outcome.block_hash;
    let effects = near
        .rpc()
        .block_effects(BlockReference::at_hash(block_hash))
        .await
        .expect("block_effects must parse");

    assert_eq!(effects.block_hash, block_hash);
    assert!(
        !effects.changes.is_empty(),
        "a block that created an account should report touched state"
    );
    // Every change kind exposes the account it touched.
    for change in &effects.changes {
        let touched = match change {
            StateChangeKindView::AccountTouched { account_id }
            | StateChangeKindView::AccessKeyTouched { account_id }
            | StateChangeKindView::DataTouched { account_id }
            | StateChangeKindView::ContractCodeTouched { account_id } => account_id,
        };
        let _ = touched;
    }
}

#[tokio::test]
async fn test_maintenance_windows_for_account() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let account: AccountId = SANDBOX_ROOT_ACCOUNT.parse().unwrap();
    // The root account is the sandbox's sole validator, so this must not error.
    // Windows may be empty depending on the epoch; the call succeeding and the
    // ranges being well-formed (start <= end) is the contract under test.
    let windows = near
        .rpc()
        .maintenance_windows(&account)
        .await
        .expect("maintenance_windows");
    for w in &windows {
        assert!(w.start <= w.end, "window range must be well-formed: {w:?}");
    }
}
