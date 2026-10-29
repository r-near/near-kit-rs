//! JSON-RPC response bodies for the client's unit tests.

use serde_json::{Value, json};

/// A successful response carrying `result`.
pub(crate) fn rpc_result(result: Value) -> Vec<u8> {
    envelope(json!({ "result": result }))
}

/// A `HANDLER_ERROR` response as nearcore sends it. `cause: None` leaves the
/// cause out, as older nodes and proxies do.
pub(crate) fn rpc_error(cause: Option<&str>, info: Value, data: Value) -> Vec<u8> {
    let mut error = json!({
        "name": "HANDLER_ERROR",
        "code": -32000,
        "message": "Server error",
        "data": data,
    });
    if let Some(cause) = cause {
        error["cause"] = json!({ "name": cause, "info": info });
    }
    envelope(json!({ "error": error }))
}

/// A `send_tx` rejected with the `InvalidTxError` `error`.
pub(crate) fn invalid_tx_body(error: Value) -> Vec<u8> {
    rpc_error(
        Some("INVALID_TRANSACTION"),
        json!({}),
        json!({ "TxExecutionError": { "InvalidTxError": error } }),
    )
}

fn envelope(mut response: Value) -> Vec<u8> {
    response["jsonrpc"] = "2.0".into();
    response["id"] = 0.into();
    serde_json::to_vec(&response).unwrap()
}

/// A thread-scoped subscriber that records every event, for tests of what
/// the client logs.
#[cfg(feature = "tracing")]
pub(crate) mod tracing_capture {
    use std::sync::{Arc, Mutex, Once};

    use tracing::field::{Field, Visit};
    use tracing::subscriber::Interest;
    use tracing::{Event, Level, Subscriber, span};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    /// A captured event: its level plus every field rendered as text.
    #[derive(Clone, Debug)]
    pub(crate) struct CapturedEvent {
        pub(crate) level: Level,
        pub(crate) fields: Vec<(&'static str, String)>,
    }

    impl CapturedEvent {
        pub(crate) fn field(&self, name: &str) -> Option<&str> {
            self.fields
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.as_str())
        }

        pub(crate) fn message(&self) -> Option<&str> {
            self.field("message")
        }
    }

    struct FieldVisitor<'a>(&'a mut Vec<(&'static str, String)>);

    impl Visit for FieldVisitor<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name(), format!("{value:?}")));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push((field.name(), value.to_string()));
        }
    }

    #[derive(Clone, Default)]
    pub(crate) struct EventLog(Arc<Mutex<Vec<CapturedEvent>>>);

    impl<S: Subscriber> Layer<S> for EventLog {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = Vec::new();
            event.record(&mut FieldVisitor(&mut fields));
            self.0.lock().unwrap().push(CapturedEvent {
                level: *event.metadata().level(),
                fields,
            });
        }
    }

    /// Permanent, do-nothing global subscriber that keeps callsites
    /// "sometimes" interested.
    ///
    /// `tracing` caches each callsite's interest process-wide. While only
    /// one dispatcher is registered, that cache is rebuilt from whichever
    /// thread's *current* dispatcher first hits the callsite; a sibling
    /// test driving the same RPC path on another thread with no subscriber
    /// pins it to `never`, and the thread-scoped `EventLog` below never
    /// sees the event (`cargo test --lib -- error` reproduced this).
    /// Registering a second dispatcher that answers `sometimes` forces the
    /// per-event `enabled` check to consult the thread-local subscriber
    /// instead, so the scoped log is honored whatever the scheduling.
    struct KeepAlive;

    impl Subscriber for KeepAlive {
        fn register_callsite(&self, _: &'static tracing::Metadata<'static>) -> Interest {
            Interest::sometimes()
        }

        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            false
        }

        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }

        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

        fn event(&self, _: &Event<'_>) {}

        fn enter(&self, _: &span::Id) {}

        fn exit(&self, _: &span::Id) {}
    }

    impl EventLog {
        /// Install as the thread-default subscriber for the current test.
        pub(crate) fn install(&self) -> tracing::subscriber::DefaultGuard {
            static KEEP_ALIVE: Once = Once::new();
            // Ignore the error: another global default is just as good
            // for our purpose (anything but the no-op default works).
            KEEP_ALIVE.call_once(|| {
                let _ = tracing::subscriber::set_global_default(KeepAlive);
            });
            tracing::subscriber::set_default(tracing_subscriber::registry().with(self.clone()))
        }

        pub(crate) fn events(&self) -> Vec<CapturedEvent> {
            self.0.lock().unwrap().clone()
        }
    }
}
