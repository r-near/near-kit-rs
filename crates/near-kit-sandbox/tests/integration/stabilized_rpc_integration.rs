//! Integration tests for the stabilized 2.13 RPC methods without a typed
//! wrapper, called through the raw `RpcClient::request`.

#[tokio::test]
async fn test_genesis_config_returns_chain_config() {
    let (_, near) = super::support::shared_client().await;

    let config: serde_json::Value = near
        .near()
        .rpc()
        .request("genesis_config", serde_json::json!([]))
        .await
        .expect("genesis_config");
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
