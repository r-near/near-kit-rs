//! Integration tests for the stabilized 2.13 RPC method wrappers:
//! `block_effects`, `genesis_config`, and `maintenance_windows`.

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

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
        config.get("protocol_version").is_some(),
        "genesis_config should include protocol_version"
    );
}

#[tokio::test]
async fn test_block_effects_returns_changes_for_latest_block() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    let effects = near
        .rpc()
        .block_effects(BlockReference::final_())
        .await
        .expect("block_effects");
    // block_hash is always present; `changes` may be empty for an idle block.
    assert_ne!(effects.block_hash, CryptoHash::default());
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
