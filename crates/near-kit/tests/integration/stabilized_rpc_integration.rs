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

    // Resolve a concrete finalized block first, then ask for its effects by
    // hash. Requesting effects for a specific block (rather than the moving
    // `final` reference) avoids a finality-resolution race under load.
    let block = near
        .rpc()
        .block(BlockReference::final_())
        .await
        .expect("block");
    let block_hash = block.header.hash;

    let effects = near
        .rpc()
        .block_effects(BlockReference::at_hash(block_hash))
        .await
        .expect("block_effects");
    // The response echoes the queried block; `changes` may be empty for an idle block.
    assert_eq!(effects.block_hash, block_hash);
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
