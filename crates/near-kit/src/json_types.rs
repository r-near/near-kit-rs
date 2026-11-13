//! near-sdk's JSON wrappers for 64- and 128-bit integers and byte vectors,
//! re-exported from `near-sdk-core`.
//!
//! Contracts take and return `u64`/`u128` amounts as decimal strings and
//! bytes as base64 strings. [`U128`] and the others are near-sdk's own
//! `json_types`, not copies: `near_kit::json_types::U128` and
//! `near_sdk::json_types::U128` are one type, so contract code and client
//! code pass values (and structs holding them) to each other with no
//! conversion. A crate shared by both can name them through
//! `near_sdk_core::json_types` or `near_kit_primitives::json_types`. In one
//! build with near-sdk, that means near-sdk 5.30.x; see
//! [`near_kit_primitives::json_types`] for the traits they have and older
//! near-sdk releases.
//!
//! Copy a contract method's signature into a `#[contract]` trait as is:
//!
#![cfg_attr(feature = "contracts", doc = "```rust")]
#![cfg_attr(not(feature = "contracts"), doc = "```rust,ignore")]
//! use near_kit::AccountId;
//! use near_kit::json_types::U128;
//!
//! #[near_kit::contract]
//! pub trait MultiToken {
//!     // {"account_id":"alice.near","token_id":"nep141:wrap.near"}, answered with "10"
//!     fn mt_balance_of(&self, account_id: &AccountId, token_id: &str) -> U128;
//!
//!     // {"receiver_id":"bob.near","token_id":"nep141:wrap.near","amount":"10",..}
//!     #[call]
//!     fn mt_transfer(
//!         &mut self,
//!         receiver_id: &AccountId,
//!         token_id: &str,
//!         amount: U128,
//!         approval: Option<(AccountId, u64)>,
//!         memo: Option<&str>,
//!     );
//! }
//! ```

#[doc(inline)]
pub use near_kit_primitives::json_types::{Base64VecU8, I64, I128, U64, U128};
