//! Integration tests verifying tracing span hierarchy.
//!
//! These tests install a capturing tracing subscriber, run real sandbox
//! operations, then assert that spans are nested correctly — i.e. that
//! child RPC/nonce spans appear inside the expected parent spans.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;
use tracing::span::Id;
use tracing_subscriber::layer::SubscriberExt;

/// Counter for generating unique subaccount names
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account() -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("trace{}.{}", n, SANDBOX_ROOT_ACCOUNT)
        .parse()
        .unwrap()
}

// =============================================================================
// Capturing Layer
// =============================================================================

/// A recorded span creation event.
#[derive(Clone, Debug)]
struct SpanRecord {
    name: &'static str,
    parent_id: Option<Id>,
}

/// Shared state for the capturing layer.
#[derive(Default, Clone, Debug)]
struct CapturedSpans {
    /// span id -> record
    spans: Arc<Mutex<HashMap<u64, SpanRecord>>>,
}

impl CapturedSpans {
    /// Return all recorded span names that are direct children of a span with
    /// the given `parent_name`.
    fn children_of(&self, parent_name: &str) -> Vec<&'static str> {
        let spans = self.spans.lock().unwrap();
        // Find all span IDs that have this name
        let parent_ids: Vec<u64> = spans
            .iter()
            .filter(|(_, r)| r.name == parent_name)
            .map(|(id, _)| *id)
            .collect();

        spans
            .iter()
            .filter(|(_, r)| {
                r.parent_id
                    .as_ref()
                    .map(|pid| parent_ids.contains(&pid.into_u64()))
                    .unwrap_or(false)
            })
            .map(|(_, r)| r.name)
            .collect()
    }

    /// Check if any span with the given name was recorded.
    fn has_span(&self, name: &str) -> bool {
        self.spans.lock().unwrap().values().any(|r| r.name == name)
    }
}

/// A tracing Layer that records span creation with parent info.
struct CapturingLayer {
    state: CapturedSpans,
}

impl<S> tracing_subscriber::Layer<S> for CapturingLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span not found");
        let parent_id = span.parent().map(|p| p.id());
        let name = span.name();
        self.state
            .spans
            .lock()
            .unwrap()
            .insert(id.into_u64(), SpanRecord { name, parent_id });
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_send_transaction_span_hierarchy() {
    let captured = CapturedSpans::default();
    let layer = CapturingLayer {
        state: captured.clone(),
    };

    // Install our capturing subscriber for this test
    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    // Create a test account — this exercises send_transaction + RPC spans
    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(account_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Verify send_transaction span was created
    assert!(
        captured.has_span("send_transaction"),
        "expected a 'send_transaction' span"
    );

    // Verify key child spans are nested under send_transaction
    let children = captured.children_of("send_transaction");
    assert!(
        children.contains(&"get_nonce"),
        "expected 'get_nonce' as child of 'send_transaction', got: {children:?}"
    );
    assert!(
        children.contains(&"send_tx"),
        "expected 'send_tx' (RPC) as child of 'send_transaction', got: {children:?}"
    );
    assert!(
        children.contains(&"view_access_key"),
        "expected 'view_access_key' as child of 'send_transaction', got: {children:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_sign_transaction_span_hierarchy() {
    let captured = CapturedSpans::default();
    let layer = CapturingLayer {
        state: captured.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();

    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account();

    // Use .sign() to exercise sign_transaction span
    let _signed = root_near
        .transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(5))
        .add_full_access_key(account_key.public_key())
        .sign()
        .await
        .unwrap();

    assert!(
        captured.has_span("sign_transaction"),
        "expected a 'sign_transaction' span"
    );

    let children = captured.children_of("sign_transaction");
    assert!(
        children.contains(&"get_nonce"),
        "expected 'get_nonce' as child of 'sign_transaction', got: {children:?}"
    );
    assert!(
        children.contains(&"block"),
        "expected 'block' as child of 'sign_transaction', got: {children:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_ft_balance_of_span_hierarchy() {
    let captured = CapturedSpans::default();
    let layer = CapturingLayer {
        state: captured.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let sandbox = SandboxConfig::shared().await;
    let root_near = sandbox.client();
    let rpc_url = sandbox.rpc_url();

    // Deploy an FT contract
    let ft_key = SecretKey::generate_ed25519();
    let ft_id = unique_account();
    let owner_id = unique_account();
    let owner_key = SecretKey::generate_ed25519();

    // Create owner account
    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::from_near(50))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Deploy FT contract
    let wasm = std::fs::read("tests/contracts/fungible_token.wasm")
        .expect("fungible_token.wasm not found");

    root_near
        .transaction(&ft_id)
        .create_account()
        .transfer(NearToken::from_near(50))
        .add_full_access_key(ft_key.public_key())
        .deploy(wasm)
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    let ft_near = Near::custom(rpc_url)
        .credentials(ft_key.to_string(), &ft_id)
        .unwrap()
        .build();

    ft_near
        .call(&ft_id, "new")
        .args(serde_json::json!({
            "owner_id": owner_id.to_string(),
            "total_supply": "1000000000000000000000000",
            "metadata": {
                "spec": "ft-1.0.0",
                "name": "Test Token",
                "symbol": "TEST",
                "decimals": 18
            }
        }))
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();

    // Clear spans from setup, we only care about balance_of
    captured.spans.lock().unwrap().clear();

    let ft = ft_near.ft(&ft_id).unwrap();
    let _balance = ft.balance_of(&owner_id).await.unwrap();

    // Verify ft_balance_of span was created
    assert!(
        captured.has_span("ft_balance_of"),
        "expected a 'ft_balance_of' span"
    );

    // The view_function RPC call should be a child of ft_balance_of
    let children = captured.children_of("ft_balance_of");
    assert!(
        children.contains(&"view_function"),
        "expected 'view_function' as child of 'ft_balance_of', got: {children:?}"
    );
}
