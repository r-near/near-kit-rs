//! A NEP-297 event built with near-kit's `events::Event` (the recipe in
//! near-kit's testing guide), or with near-sdk-core alone through a
//! hand-written `AsNep297Event`, logs the same line as near-sdk's
//! `#[near(event_json)]` for the same enum, and near-kit parses near-sdk's
//! line back to an equal value.

use near_kit::events::{Event, RawEvent};
use near_sdk::test_utils::get_logs;
use near_sdk_core::events::{AsNep297Event, Nep297Event};
use near_sdk_core::json_types::U128;
use serde::{Deserialize, Serialize};

/// The client side: a plain serde enum, with the NEP-297 standard and each
/// variant's version written by hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum ClientEvent {
    PublicKeyAdded {
        account_id: String,
        public_key: String,
    },
    FeeChanged {
        old_fee: U128,
        new_fee: U128,
    },
    Paused,
}

impl AsNep297Event for ClientEvent {
    fn to_nep297_event(&self) -> Nep297Event<'_, Self> {
        let (version, event) = match self {
            Self::PublicKeyAdded { .. } => ("0.4.3", "public_key_added"),
            Self::FeeChanged { .. } => ("0.3.0", "fee_changed"),
            Self::Paused => ("1.0.0", "paused"),
        };
        Nep297Event::new("dip4", version, event, self)
    }
}

impl ClientEvent {
    /// The event as near-kit builds it: the standard and the variant's
    /// version, with the enum as the payload.
    fn into_event(self) -> Event<Self> {
        let version = match self {
            Self::PublicKeyAdded { .. } => "0.4.3",
            Self::FeeChanged { .. } => "0.3.0",
            Self::Paused => "1.0.0",
        };
        Event::new("dip4", version, self)
    }
}

/// The contract side: the same enum through near-sdk's derive.
mod contract {
    use near_sdk::json_types::U128;
    use near_sdk::near;

    #[near(event_json(standard = "dip4"))]
    pub enum ContractEvent {
        #[event_version("0.4.3")]
        PublicKeyAdded {
            account_id: String,
            public_key: String,
        },
        #[event_version("0.3.0")]
        FeeChanged { old_fee: U128, new_fee: U128 },
        #[event_version("1.0.0")]
        Paused,
    }
}

use contract::ContractEvent;

/// The line `event.emit()` logs in a near-sdk contract.
fn emitted(event: ContractEvent) -> String {
    near_sdk::testing_env!(near_sdk::test_utils::VMContextBuilder::new().build());
    event.emit();
    let logs = get_logs();
    assert_eq!(logs.len(), 1, "{logs:?}");
    logs.into_iter().next().unwrap()
}

#[test]
fn near_sdk_derives_near_sdk_cores_trait() {
    fn core_event<T: AsNep297Event>() {}
    core_event::<ContractEvent>();
}

/// `Nep297Event` takes `event` from the payload's own `event` key: a payload
/// that isn't a tag/content enum logs no `event` and no `data`
/// (near/near-sdk-rs#1680).
#[test]
fn a_struct_payload_loses_event_and_data() {
    #[derive(Serialize)]
    struct Plain {
        amount: u32,
    }
    assert_eq!(
        Nep297Event::new("nep999", "1.0.0", "my_event", &Plain { amount: 5 }).to_event_log(),
        r#"EVENT_JSON:{"standard":"nep999","version":"1.0.0","amount":5}"#
    );
}

/// Each variant on both sides: a struct variant, one with `U128` fields
/// (including `u128::MAX`) and a unit variant.
fn both_sides() -> Vec<(ClientEvent, ContractEvent)> {
    let key = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp";
    vec![
        (
            ClientEvent::PublicKeyAdded {
                account_id: "alice.near".into(),
                public_key: key.into(),
            },
            ContractEvent::PublicKeyAdded {
                account_id: "alice.near".into(),
                public_key: key.into(),
            },
        ),
        (
            ClientEvent::FeeChanged {
                old_fee: U128(5),
                new_fee: U128(u128::MAX),
            },
            ContractEvent::FeeChanged {
                old_fee: U128(5),
                new_fee: U128(u128::MAX),
            },
        ),
        (ClientEvent::Paused, ContractEvent::Paused),
    ]
}

#[test]
fn near_kit_event_logs_what_near_sdk_emits() {
    for (client, sdk) in both_sides() {
        let expected = client.clone().into_event();
        let to_event_log = near_sdk::AsNep297Event::to_nep297_event(&sdk).to_event_log();
        assert_eq!(expected.to_log(), to_event_log);
        assert_eq!(expected.to_log(), emitted(sdk));
        // near-sdk-core's envelope on the client enum agrees too.
        assert_eq!(expected.to_log(), client.to_nep297_event().to_event_log());
    }
    assert_eq!(
        ClientEvent::Paused.into_event().to_log(),
        r#"EVENT_JSON:{"standard":"dip4","version":"1.0.0","event":"paused"}"#
    );
}

#[test]
fn near_kit_reads_what_near_sdk_emits() {
    for (client, sdk) in both_sides() {
        let expected = client.into_event();
        let log = emitted(sdk);
        assert_eq!(
            Event::<ClientEvent>::from_log(&log).unwrap().unwrap(),
            expected
        );

        let raw: Event<RawEvent> = Event::from_log(&log).unwrap().unwrap();
        assert_eq!(raw.standard, "dip4");
        assert_eq!(raw.version, expected.version);
        assert_eq!(raw.decode::<ClientEvent>().unwrap(), expected);
    }
}
