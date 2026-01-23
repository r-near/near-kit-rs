//! Typed contract interfaces.
//!
//! This module provides the `Contract` trait and types for creating type-safe
//! contract clients using the `#[near_kit::contract]` proc macro.
//!
//! # Example
//!
//! ```ignore
//! use near_kit::*;
//! use serde::Serialize;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     fn get_count(&self) -> u64;
//!     
//!     #[call]
//!     fn increment(&mut self);
//!     
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//! }
//!
//! #[derive(Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//!
//! async fn example(near: &Near) -> Result<(), Error> {
//!     let counter = near.contract::<Counter>("counter.testnet");
//!     let count = counter.get_count().await?;
//!     counter.increment().await?;
//!     Ok(())
//! }
//! ```

use crate::client::Near;
use crate::types::AccountId;

/// Marker trait for typed contract interfaces.
///
/// This trait is automatically implemented by the `#[near_kit::contract]` macro
/// for each contract trait you define. It provides the associated `Client` type
/// that is used by [`Near::contract`](crate::Near::contract).
///
/// # Example
///
/// The macro generates an implementation like this:
///
/// ```ignore
/// impl Contract for dyn MyContract {
///     type Client<'a> = MyContractClient<'a>;
/// }
/// ```
pub trait Contract {
    /// The generated client type for this contract interface.
    type Client<'a>: ContractClient<'a>;
}

/// Trait for contract client constructors.
///
/// This trait is implemented by the generated client structs to enable
/// construction via [`Near::contract`](crate::Near::contract).
pub trait ContractClient<'a>: Sized {
    /// Create a new contract client.
    fn new(near: &'a Near, contract_id: AccountId) -> Self;
}
