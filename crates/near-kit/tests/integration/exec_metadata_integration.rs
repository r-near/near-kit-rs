//! Integration test for parsing ExecutionMetadata V4 (protocol 2.13) through
//! the high-level outcome path.
//!
//! Before V4 was modeled, a 2.13 receipt's `metadata: {version: 4, contracts:
//! [...]}` could break `FinalExecutionOutcome` deserialization: the
//! `#[serde(flatten)] Option<FinalExecutionOutcome>` silently becomes `None`
//! when any nested field fails to parse, surfacing as "no execution outcome".
//! This test confirms a real 2.13 outcome round-trips through the typed
//! `.send().wait_until(Final)` path and exposes the V4 `contracts` field.

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("execmeta{}.{}", n, SANDBOX_ROOT_ACCOUNT)
        .parse()
        .unwrap()
}

#[tokio::test]
async fn test_v4_execution_metadata_parses_through_send() {
    let sandbox = SandboxConfig::shared().await;
    let near = sandbox.client();

    // A create_account + transfer produces receipt outcomes on a 2.13 node.
    // If V4 metadata didn't parse, `.send()` would fail to deserialize the
    // outcome entirely (the bug this guards against).
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account();
    let outcome = near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(5))
        .add_full_access_key(key.public_key())
        .send()
        .wait_until(Final)
        .await
        .expect("create_account outcome must deserialize (incl. V4 metadata)");

    // The transaction-level outcome always carries metadata; assert it parsed.
    let tx_meta = outcome
        .transaction_outcome
        .outcome
        .metadata
        .as_ref()
        .expect("transaction outcome should carry metadata");
    assert!(tx_meta.version >= 1);

    // On a protocol-v85+ node the receipt outcomes carry V4 metadata, whose
    // typed `contracts` field must be exposed (one entry per action).
    let protocol = near.rpc().status().await.expect("status").protocol_version;
    if protocol >= 85 {
        let v4 = outcome
            .receipts_outcome
            .iter()
            .filter_map(|r| r.outcome.metadata.as_ref())
            .find(|m| m.version >= 4);
        let v4 = v4.expect("expected a V4 ExecutionMetadata on a v85+ node");
        assert!(
            v4.contracts.is_some(),
            "V4 metadata must expose the typed `contracts` field"
        );
    }
}
