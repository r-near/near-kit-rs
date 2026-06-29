//! Integration tests for the `view_state` RPC method and its pagination.
//!
//! # Sandbox image
//!
//! `view_state` pagination (`after_key_base64` / `limit` / `last_key`) is newer
//! than the default `2.13.0-rc.2` image, which ignores `limit` and returns the
//! whole state in one page. We therefore run against the `pre-release` tag,
//! which includes pagination. Drop the override once a 2.13 RC ships with it.

use near_kit::sandbox::{Sandbox, SandboxConfig};
use near_kit::*;

/// Sandbox image tag that supports `view_state` pagination.
const VIEW_STATE_SANDBOX_VERSION: &str = "pre-release";

/// A fresh sandbox running a node that supports `view_state` pagination.
async fn paginating_sandbox() -> Sandbox {
    SandboxConfig::builder()
        .version(VIEW_STATE_SANDBOX_VERSION)
        .fresh()
        .await
}

/// Deploy the guestbook contract to the given (freshly created) account.
async fn deploy_guestbook(near: &Near, contract_account: &str) {
    let wasm_code = std::fs::read("tests/contracts/guestbook.wasm")
        .expect("guestbook.wasm not found in tests/contracts/");
    let new_key = SecretKey::generate_ed25519();
    near.transaction(contract_account)
        .create_account()
        .transfer("10 NEAR")
        .add_full_access_key(new_key.public_key())
        .deploy(wasm_code)
        .send()
        .wait_until(Final)
        .await
        .expect("deploy guestbook");
}

#[tokio::test]
async fn test_view_state_pagination_reads_all_entries() {
    let sandbox = paginating_sandbox().await;
    let near = Near::sandbox(&sandbox);

    let contract_id = format!("vstate.{}", sandbox.root_account_id());
    deploy_guestbook(&near, &contract_id).await;

    // Populate state: each message extends the on-trie Vector, adding entries.
    const N: usize = 8;
    for i in 0..N {
        near.transaction(&contract_id)
            .call("add_message")
            .args(serde_json::json!({ "text": format!("message number {i}") }))
            .send()
            .wait_until(Final)
            .await
            .expect("add_message");
    }

    let contract: AccountId = contract_id.parse().unwrap();

    // Single-page call with a small limit must return a continuation cursor.
    let first = near
        .rpc()
        .view_state(&contract, &[], None, Some(2), BlockReference::final_())
        .await
        .expect("view_state page");
    assert!(!first.values.is_empty());
    assert!(first.values.len() <= 2, "limit must be honored");
    assert!(
        first.last_key.is_some(),
        "with more entries than the limit, a cursor must be returned"
    );

    // Following the cursor yields different entries than the first page.
    let second = near
        .rpc()
        .view_state(
            &contract,
            &[],
            first.last_key.as_deref(),
            Some(2),
            BlockReference::final_(),
        )
        .await
        .expect("view_state page 2");
    assert!(!second.values.is_empty());
    assert_ne!(
        first.values[0].key, second.values[0].key,
        "second page should start after the first page's cursor"
    );

    // The paginating helper must collect every entry, matching an unpaginated read.
    let all_paged = near
        .rpc()
        .view_state_all(&contract, &[], 2, BlockReference::final_())
        .await
        .expect("view_state_all paged");
    let all_unpaged = near
        .rpc()
        .view_state_all(&contract, &[], 0, BlockReference::final_())
        .await
        .expect("view_state_all unpaged");

    assert_eq!(
        all_paged, all_unpaged,
        "paginated and single-shot reads must agree"
    );
    assert!(
        all_paged.len() >= N,
        "expected at least {N} state entries, got {}",
        all_paged.len()
    );

    // Prefix filtering: every returned key must start with the requested prefix.
    let prefix = &all_paged[0].key[..1];
    let filtered = near
        .rpc()
        .view_state_all(&contract, prefix, 0, BlockReference::final_())
        .await
        .expect("view_state_all prefix");
    assert!(!filtered.is_empty());
    assert!(filtered.iter().all(|item| item.key.starts_with(prefix)));
}
