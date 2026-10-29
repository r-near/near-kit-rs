//! NEAR token amount and gas unit types, re-exported from upstream crates.
//!
//! Both parse from strings with their upstream `FromStr` impls
//! (`"5 NEAR".parse::<NearToken>()`, `"30 Tgas".parse::<Gas>()`); methods
//! take them typed.

pub use near_gas::NearGas as Gas;
pub use near_token::NearToken;
