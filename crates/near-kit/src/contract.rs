//! Typed contract interfaces.
//!
//! This module provides the [`Contract`] trait for creating type-safe contract
//! clients using the `#[near_kit::contract]` proc macro.
//!
//! # Why Typed Contracts?
//!
//! Without typed contracts, method names and arguments are stringly-typed:
//!
//! ```rust,no_run
//! # use near_kit::*;
//! # async fn example(near: &Near) -> Result<(), Error> {
//! // Typos compile fine but fail at runtime
//! let count: u64 = near.view("counter.near", "get_counnt").await?;  // typo!
//! # Ok(())
//! # }
//! ```
//!
//! With typed contracts, the compiler catches errors:
//!
//! ```ignore
//! let counter = near.contract::<Counter>("counter.near");
//! let count = counter.get_count().await?;  // Compile-time checked!
//! ```
//!
//! # Defining a Contract Interface
//!
//! Use the `#[near_kit::contract]` macro on a trait:
//!
//! ```ignore
//! use near_kit::*;
//! use serde::Serialize;
//!
//! #[near_kit::contract]
//! pub trait Counter {
//!     // View method: &self, no #[call] attribute
//!     fn get_count(&self) -> u64;
//!
//!     // Change method: &mut self + #[call] attribute
//!     #[call]
//!     fn increment(&mut self);
//!
//!     // Change method with arguments
//!     #[call]
//!     fn add(&mut self, args: AddArgs);
//!
//!     // Payable method (can receive NEAR deposit)
//!     #[call(payable)]
//!     fn donate(&mut self);
//! }
//!
//! #[derive(Serialize)]
//! pub struct AddArgs {
//!     pub value: u64,
//! }
//! ```
//!
//! # Using a Typed Contract
//!
//! ```ignore
//! async fn example(near: &Near) -> Result<(), Error> {
//!     let counter = near.contract::<Counter>("counter.testnet");
//!
//!     // View calls
//!     let count = counter.get_count().await?;
//!
//!     // Change calls
//!     counter.increment().await?;
//!     counter.add(AddArgs { value: 5 }).await?;
//!
//!     // Payable calls with deposit
//!     counter.donate().deposit(NearToken::near(1)).await?;
//!
//!     // Override gas
//!     counter.add(AddArgs { value: 10 }).gas(Gas::tgas(50)).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Serialization Formats
//!
//! By default, arguments are serialized as JSON. For Borsh serialization:
//!
//! ```ignore
//! #[near_kit::contract(borsh)]
//! pub trait MyContract {
//!     fn my_method(&self, args: MyArgs) -> u64;
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
