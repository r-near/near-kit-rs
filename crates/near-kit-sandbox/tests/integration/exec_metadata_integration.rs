//! Integration test for parsing ExecutionMetadata V4 (protocol 2.13) through
//! the high-level outcome path.
//!
//! Before V4 was modeled, a 2.13 receipt's `metadata: {version: 4, contracts:
//! [...]}` could break `FinalExecutionOutcome` deserialization: the
//! `#[serde(flatten)] Option<FinalExecutionOutcome>` silently becomes `None`
//! when any nested field fails to parse, surfacing as "no execution outcome".
//! This test confirms a real 2.13+ outcome round-trips through the typed
//! `wait_until::<Final>().transaction(..)` path and exposes the V4
//! `contracts` field.

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{signer::SecretKey, transaction::Final};

use super::support::unique_account;

#[tokio::test]
async fn test_v4_execution_metadata_parses_through_send() {
    let (_, root) = super::support::shared_client().await;

    // A create_account + transfer produces receipt outcomes on a 2.13 node.
    // If V4 metadata didn't parse, the send would fail to deserialize the
    // outcome entirely (the bug this guards against).
    let key = SecretKey::generate_ed25519();
    let account_id = unique_account("execmeta");
    let outcome = root
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(5)))
        .add_action(Action::add_full_access_key(key.public_key()))
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

    // The default sandbox is past protocol v85, so the receipt outcomes carry
    // V4 metadata, whose typed `contracts` field must be exposed.
    let v4 = outcome
        .receipts_outcome
        .iter()
        .filter_map(|r| r.outcome.metadata.as_ref())
        .find(|m| m.version >= 4)
        .expect("expected a V4 ExecutionMetadata on a v85+ node");
    assert!(
        v4.contracts.is_some(),
        "V4 metadata must expose the typed `contracts` field"
    );
}
