//! Integration tests for the `view_state` RPC method and its pagination.
//!
//! # Sandbox image
//!
//! Pins the sandbox to stable `2.13.4` and verifies `view_state` pagination
//! (`after_key_base64` / `limit` / `last_key`).

use super::support::{guestbook_wasm, sandbox_2_13};
use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_kit::*;
use near_kit::{rpc::BlockReference, signer::SecretKey, transaction::Final};

/// Deploy the guestbook contract to the given (freshly created) account.
async fn deploy_guestbook(near: &SigningNear, contract_account: &AccountId) {
    let wasm_code = guestbook_wasm();
    let new_key = SecretKey::generate_ed25519();
    near.wait_until::<Final>()
        .transaction(contract_account)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(new_key.public_key()))
        .add_action(Action::deploy_contract(wasm_code))
        .await
        .expect("deploy guestbook")
        .result()
        .unwrap();
}

#[tokio::test]
async fn test_view_state_pagination_reads_all_entries() {
    let sandbox = sandbox_2_13().await;
    let near = sandbox.root_account();

    let contract_id: AccountId = format!("vstate.{}", near.account_id()).parse().unwrap();
    deploy_guestbook(&near, &contract_id).await;

    // Populate state: each message extends the on-trie Vector, adding entries.
    const N: usize = 8;
    for i in 0..N {
        near.wait_until::<Final>()
            .transaction(&contract_id)
            .add_action(
                FunctionCall::new("add_message")
                    .args(serde_json::json!({ "text": format!("message number {i}") })),
            )
            .await
            .expect("add_message")
            .result()
            .unwrap();
    }

    let contract = contract_id.clone();

    // Single-page call with a small limit must return a continuation cursor.
    let first = near
        .near()
        .rpc()
        .view_state(&contract, &[], None, Some(2), BlockReference::default())
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
        .near()
        .rpc()
        .view_state(
            &contract,
            &[],
            first.last_key.as_deref(),
            Some(2),
            BlockReference::default(),
        )
        .await
        .expect("view_state page 2");
    assert!(!second.values.is_empty());
    assert_ne!(
        first.values[0].key, second.values[0].key,
        "second page should start after the first page's cursor"
    );

    // The paginating helper must collect every entry regardless of page size:
    // a small explicit limit vs. the default page size (page_size = 0).
    let all_small_pages = near
        .near()
        .rpc()
        .view_state_all(&contract, &[], 2, BlockReference::default())
        .await
        .expect("view_state_all with small pages");
    let all_default_page = near
        .near()
        .rpc()
        .view_state_all(&contract, &[], 0, BlockReference::default())
        .await
        .expect("view_state_all with default page size");

    assert_eq!(
        all_small_pages.values, all_default_page.values,
        "small-page and node-default-page reads must collect the same entries"
    );
    assert!(
        all_small_pages.values.len() >= N,
        "expected at least {N} state entries, got {}",
        all_small_pages.values.len()
    );
    // The paginated read reports the block it was pinned to, like every other
    // typed view. The two reads resolved `final` separately so their blocks
    // may differ; check the reported hash/height agree with the node instead.
    assert!(all_small_pages.block_height > 0);
    let pinned = near
        .near()
        .rpc()
        .block(BlockReference::Hash(all_small_pages.block_hash))
        .await
        .expect("block view_state_all was pinned to");
    assert_eq!(pinned.header.height, all_small_pages.block_height);

    // Prefix filtering: every returned key must start with the requested prefix.
    let prefix = &all_small_pages.values[0].key[..1];
    let filtered = near
        .near()
        .rpc()
        .view_state_all(&contract, prefix, 0, BlockReference::default())
        .await
        .expect("view_state_all prefix");
    assert!(!filtered.values.is_empty());
    assert!(
        filtered
            .values
            .iter()
            .all(|item| item.key.starts_with(prefix))
    );
}
