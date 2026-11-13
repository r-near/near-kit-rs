//! [NEP-297](https://github.com/near/NEPs/blob/master/neps/nep-0297.md)
//! events: build the `EVENT_JSON:` line a contract logs, and read event logs
//! back as typed values, with no near-sdk.
//!
//! [`FinalExecutionOutcome::events`](crate::rpc::FinalExecutionOutcome::events)
//! reads a transaction's events. See [`near_kit_primitives::events`] for an
//! example, and the testing guide's
//! [Checking NEP-297 events](https://docs.rs/near-kit/0.30.0-rc.9/near_kit/guide/testing/index.html#checking-nep-297-events)
//! for checking them in a test.

#[doc(inline)]
pub use near_kit_primitives::events::{Event, RawEvent};
