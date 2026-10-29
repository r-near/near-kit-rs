//! Integration tests verifying tracing span hierarchy.
//!
//! These tests install a capturing tracing subscriber, run real sandbox
//! operations, then assert that spans are nested correctly — i.e. that
//! child RPC/nonce spans appear inside the expected parent spans.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::standards::FungibleToken;
use near_kit::transaction::FunctionCall;
use near_kit::*;
use near_kit::{signer::*, transaction::Final};
use tracing::span::Id;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::layer::SubscriberExt;

use super::support::{deploy_ft_contract, funded_account, unique_account};

/// A recorded span creation event.
#[derive(Clone, Debug)]
struct SpanRecord {
    name: &'static str,
    parent_id: Option<Id>,
    fields: HashMap<&'static str, String>,
}

/// Shared state for the capturing layer.
#[derive(Default, Clone, Debug)]
struct CapturedSpans {
    /// span id -> record
    spans: Arc<Mutex<HashMap<u64, SpanRecord>>>,
}

/// Visitor that collects span fields as strings.
#[derive(Default)]
struct FieldVisitor {
    fields: HashMap<&'static str, String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(field.name(), format!("{:?}", value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields.insert(field.name(), value.to_string());
    }
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

    /// Find a span by name and return its fields.
    fn find_span_fields(&self, name: &str) -> Option<HashMap<&'static str, String>> {
        self.spans
            .lock()
            .unwrap()
            .values()
            .find(|r| r.name == name)
            .map(|r| r.fields.clone())
    }
}

/// A tracing Layer that records span creation with parent info and fields.
struct CapturingLayer {
    state: CapturedSpans,
}

impl<S> tracing_subscriber::Layer<S> for CapturingLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("span not found");
        let parent_id = span.parent().map(|p| p.id());
        let name = span.name();

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);

        self.state.spans.lock().unwrap().insert(
            id.into_u64(),
            SpanRecord {
                name,
                parent_id,
                fields: visitor.fields,
            },
        );
    }

    fn on_record(
        &self,
        id: &Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);

        if let Some(record) = self.state.spans.lock().unwrap().get_mut(&id.into_u64()) {
            record.fields.extend(visitor.fields);
        }
    }
}

/// Install a span-capturing subscriber for the current thread.
fn capture() -> (CapturedSpans, DefaultGuard) {
    let captured = CapturedSpans::default();
    let layer = CapturingLayer {
        state: captured.clone(),
    };
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
    (captured, guard)
}

#[tokio::test(flavor = "current_thread")]
async fn test_send_transaction_span_hierarchy() {
    let (captured, _guard) = capture();
    let (sandbox, root_near) = super::support::shared_client().await;

    // Creating an account exercises send_transaction + RPC spans
    funded_account(&root_near, sandbox, "trace", NearToken::from_near(10)).await;

    let children = captured.children_of("send_transaction");
    assert!(
        children.contains(&"send_tx"),
        "expected 'send_tx' (RPC) as child of 'send_transaction', got: {children:?}"
    );
    assert!(
        children.contains(&"view_access_key"),
        "expected 'view_access_key' as child of 'send_transaction', got: {children:?}"
    );

    let fields = captured
        .find_span_fields("send_transaction")
        .expect("send_transaction span should exist");
    assert_eq!(
        fields.get("actions").map(|s| s.as_str()),
        Some("create_account,transfer,add_key"),
        "expected actions summary for create+transfer+add_key transaction"
    );
    // No function call → method/gas/deposit should not be recorded
    assert!(
        !fields.contains_key("method"),
        "method should not be set for non-function-call transaction"
    );

    let rpc_fields = captured
        .find_span_fields("request")
        .expect("RPC call span should exist");
    assert!(
        rpc_fields.contains_key("rpc.url"),
        "expected rpc.url field on RPC call span, got: {rpc_fields:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_sign_transaction_span_hierarchy() {
    let (captured, _guard) = capture();
    let (_, root_near) = super::support::shared_client().await;

    let account_key = SecretKey::generate_ed25519();
    let account_id = unique_account("trace");

    root_near
        .sign(
            &account_id,
            vec![
                Action::create_account(),
                Action::transfer(NearToken::from_near(5)),
                Action::add_full_access_key(account_key.public_key()),
            ],
        )
        .await
        .unwrap();

    let children = captured.children_of("sign_transaction");
    assert!(
        children.contains(&"view_access_key"),
        "expected 'view_access_key' as child of 'sign_transaction', got: {children:?}"
    );

    let fields = captured
        .find_span_fields("sign_transaction")
        .expect("sign_transaction span should exist");
    assert_eq!(
        fields.get("actions").map(|s| s.as_str()),
        Some("create_account,transfer,add_key"),
        "expected actions summary on sign_transaction span"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_function_call_span_fields() {
    let (captured, _guard) = capture();
    let (_, root_near) = super::support::shared_client().await;

    let (ft_id, _) = deploy_ft_contract(&root_near, root_near.account_id())
        .await
        .unwrap();

    // Clear setup spans — we only care about the function call
    captured.spans.lock().unwrap().clear();

    root_near
        .wait_until::<Final>()
        .transaction(&ft_id)
        .add_action(
            FunctionCall::new("storage_deposit")
                .args(serde_json::json!({}))
                .deposit(NearToken::from_millinear(10)),
        )
        .await
        .unwrap()
        .result()
        .unwrap();

    let fields = captured
        .find_span_fields("send_transaction")
        .expect("send_transaction span should exist");
    assert_eq!(
        fields.get("actions").map(|s| s.as_str()),
        Some("function_call(storage_deposit)"),
        "expected actions summary for function call transaction"
    );

    // Single function call → method, gas, deposit should be recorded as span fields
    assert_eq!(
        fields.get("method").map(|s| s.as_str()),
        Some("storage_deposit"),
        "expected method field for single function call"
    );
    assert!(
        fields.contains_key("gas"),
        "expected gas field for single function call, got: {fields:?}"
    );
    assert!(
        fields.contains_key("deposit"),
        "expected deposit field for single function call, got: {fields:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_ft_balance_of_span_hierarchy() {
    let (captured, _guard) = capture();
    let (_, root_near) = super::support::shared_client().await;

    let owner_id = root_near.account_id().clone();
    let (ft_id, _) = deploy_ft_contract(&root_near, &owner_id).await.unwrap();

    // Clear spans from setup, we only care about balance_of
    captured.spans.lock().unwrap().clear();

    let ft = root_near.contract::<FungibleToken>(&ft_id);
    ft.balance_of(&owner_id).await.unwrap();

    // The view_function RPC call should be a child of ft_balance_of
    let children = captured.children_of("ft_balance_of");
    assert!(
        children.contains(&"view_function"),
        "expected 'view_function' as child of 'ft_balance_of', got: {children:?}"
    );
}
