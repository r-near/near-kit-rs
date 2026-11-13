//! [NEP-297](https://github.com/near/NEPs/blob/master/neps/nep-0297.md)
//! events: build the `EVENT_JSON:` line a contract logs, and read event logs
//! back as typed values.
//!
//! [`Event<T>`] is the JSON after `EVENT_JSON:`. `T` holds the `event` name
//! and `data`: your own enum, written as near-sdk's `#[near(event_json(..))]`
//! writes it, or [`RawEvent`] to keep them undecoded.
//! [`FinalExecutionOutcome::events`](crate::rpc::FinalExecutionOutcome::events)
//! reads a transaction's events.
//!
//! ```rust
//! use near_kit_primitives::events::{Event, RawEvent};
//! use near_kit_primitives::json_types::U128;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! #[serde(tag = "event", content = "data", rename_all = "snake_case")]
//! enum VaultEvent {
//!     Deposited { amount: U128 },
//! }
//!
//! let event = Event::new("vault", "1.1.0", VaultEvent::Deposited { amount: U128(10) });
//! let log = event.to_log();
//! assert_eq!(
//!     log,
//!     r#"EVENT_JSON:{"standard":"vault","version":"1.1.0","event":"deposited","data":{"amount":"10"}}"#
//! );
//! assert_eq!(Event::<VaultEvent>::from_log(&log).unwrap()?, event);
//!
//! let raw: Event<RawEvent> = Event::from_log(&log).unwrap()?;
//! assert_eq!(raw.payload.event, "deposited");
//! assert_eq!(raw.decode::<VaultEvent>()?, event);
//! # Ok::<(), serde_json::Error>(())
//! ```

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const EVENT_LOG_PREFIX: &str = "EVENT_JSON:";

/// A NEP-297 event: the JSON after `EVENT_JSON:` in a receipt log.
///
/// `payload` is flattened next to `standard` and `version`, and holds the
/// `event` name and `data`: a `#[serde(tag = "event", content = "data")]`
/// enum with snake_case names, as near-sdk's `#[near(event_json(..))]`
/// writes it, or [`RawEvent`]. Deserializing needs `standard`, `version` and
/// `event` and ignores keys NEP-297 doesn't define.
///
/// The fields are public and the struct isn't `#[non_exhaustive]` on
/// purpose: NEP-297 fixes the envelope to these keys, and tests build the
/// events they expect as struct literals. Which account logged an event
/// isn't part of it; [`FinalExecutionOutcome::events`] filters by that.
///
/// `T` must write the `event` key itself. A plain struct payload doesn't:
/// `Event::new("nep999", "1.0.0", Plain { amount: 5 })` serializes to
/// `{"standard":"nep999","version":"1.0.0","amount":5}`, which isn't a
/// NEP-297 event (near-sdk-core's `Nep297Event` does the same,
/// [near/near-sdk-rs#1680](https://github.com/near/near-sdk-rs/issues/1680)).
/// Wrap a struct payload in a one-variant enum.
///
/// [`FinalExecutionOutcome::events`]: crate::rpc::FinalExecutionOutcome::events
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event<T = RawEvent> {
    /// The standard, such as `"nep141"`.
    pub standard: String,
    /// The standard's version, such as `"1.0.0"`.
    pub version: String,
    /// The `event` name and `data`.
    #[serde(flatten)]
    pub payload: T,
}

/// An event's name and data, not decoded. A `"data": null` reads as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEvent {
    /// The event's name, such as `"ft_transfer"`.
    pub event: String,
    /// The event's data, if it has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl<T> Event<T> {
    /// An event of `standard` at `version`.
    pub fn new(standard: impl Into<String>, version: impl Into<String>, payload: T) -> Self {
        Self {
            standard: standard.into(),
            version: version.into(),
            payload,
        }
    }
}

impl<T: Serialize> Event<T> {
    /// The log line a contract writes for this event:
    /// `EVENT_JSON:{"standard":..,"version":..,"event":..,"data":..}`. For a
    /// tag/content enum it is byte-for-byte the line near-sdk 5.30 logs.
    ///
    /// # Panics
    ///
    /// If `T`'s `Serialize` impl fails, which serde's derives don't.
    pub fn to_log(&self) -> String {
        let json = serde_json::to_string(self).expect("an event serializes to JSON");
        format!("{EVENT_LOG_PREFIX}{json}")
    }
}

impl<T: DeserializeOwned> Event<T> {
    /// Parses a log line: `None` if it doesn't start with `EVENT_JSON:`,
    /// `Some(Err)` if it does but isn't an `Event<T>`.
    pub fn from_log(log: &str) -> Option<Result<Self, serde_json::Error>> {
        log.strip_prefix(EVENT_LOG_PREFIX).map(serde_json::from_str)
    }
}

impl Event<RawEvent> {
    /// The event with its payload decoded as `T`, typically after checking
    /// [`standard`](Self::standard).
    pub fn decode<T: DeserializeOwned>(self) -> Result<Event<T>, serde_json::Error> {
        serde_json::from_value(serde_json::to_value(self)?)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::json_types::U128;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "event", content = "data", rename_all = "snake_case")]
    enum TestEvent {
        FeeChanged { old_fee: U128, new_fee: U128 },
        Paused,
    }

    const FEE_CHANGED: &str = r#"EVENT_JSON:{"standard":"dip4","version":"0.3.0","event":"fee_changed","data":{"old_fee":"5","new_fee":"340282366920938463463374607431768211455"}}"#;
    const PAUSED: &str = r#"EVENT_JSON:{"standard":"dip4","version":"1.0.0","event":"paused"}"#;

    fn fee_changed() -> Event<TestEvent> {
        let (old_fee, new_fee) = (U128(5), U128(u128::MAX));
        Event::new("dip4", "0.3.0", TestEvent::FeeChanged { old_fee, new_fee })
    }

    #[test]
    fn typed_events_round_trip_through_their_log() {
        let paused = Event::new("dip4", "1.0.0", TestEvent::Paused);
        for (event, log) in [(fee_changed(), FEE_CHANGED), (paused, PAUSED)] {
            assert_eq!(event.to_log(), log);
            assert_eq!(Event::from_log(log).unwrap().unwrap(), event);
        }
    }

    #[test]
    fn raw_events_decode_like_typed_ones() {
        let raw: Event = Event::from_log(FEE_CHANGED).unwrap().unwrap();
        assert_eq!(raw.payload.event, "fee_changed");
        assert_eq!(raw.decode::<TestEvent>().unwrap(), fee_changed());

        // No `data`, a null `data` and unknown keys all read as the same event.
        let expected = Event::new(
            "dip4",
            "1.0.0",
            RawEvent {
                event: "paused".into(),
                data: None,
            },
        );
        for log in [
            PAUSED,
            r#"EVENT_JSON:{"standard":"dip4","version":"1.0.0","event":"paused","data":null}"#,
            r#"EVENT_JSON:{"standard":"dip4","version":"1.0.0","event":"paused","extra":1}"#,
        ] {
            let raw: Event = Event::from_log(log).unwrap().unwrap();
            assert_eq!(raw, expected);
            assert_eq!(raw.to_log(), PAUSED);
            let typed = Event::new("dip4", "1.0.0", TestEvent::Paused);
            assert_eq!(Event::from_log(log).unwrap().unwrap(), typed);
            assert_eq!(raw.decode::<TestEvent>().unwrap(), typed);
        }
    }

    #[test]
    fn other_lines_are_none_and_malformed_events_are_errors() {
        assert!(Event::<RawEvent>::from_log("Transferred 10").is_none());
        assert!(Event::<RawEvent>::from_log(&format!(" {PAUSED}")).is_none());
        for log in [
            "EVENT_JSON:invalid json",
            r#"EVENT_JSON:{"version":"1.0.0","event":"paused"}"#,
            r#"EVENT_JSON:{"standard":"dip4","event":"paused"}"#,
            r#"EVENT_JSON:{"standard":"dip4","version":"1.0.0"}"#,
        ] {
            assert!(Event::<RawEvent>::from_log(log).unwrap().is_err(), "{log}");
        }
        // Another standard's event isn't a `TestEvent`.
        let transfer =
            r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"ft_transfer","data":[]}"#;
        assert!(Event::<TestEvent>::from_log(transfer).unwrap().is_err());
        let raw: Event = Event::from_log(transfer).unwrap().unwrap();
        assert!(raw.decode::<TestEvent>().is_err());
    }

    #[test]
    fn a_struct_payload_writes_no_event_key() {
        #[derive(Serialize)]
        struct Plain {
            amount: u32,
        }
        assert_eq!(
            Event::new("nep999", "1.0.0", Plain { amount: 5 }).to_log(),
            r#"EVENT_JSON:{"standard":"nep999","version":"1.0.0","amount":5}"#
        );
    }
}
