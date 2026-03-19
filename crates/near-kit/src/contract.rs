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
//!     counter.donate().deposit(NearToken::from_near(1)).await?;
//!
//!     // Override gas
//!     counter.add(AddArgs { value: 10 }).gas(Gas::from_tgas(50)).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Composing Typed Calls in Transactions
//!
//! The macro also generates static `FunctionCall` constructors on the struct,
//! so typed calls can be mixed with other actions in a single transaction:
//!
//! ```ignore
//! // Compose state_init + typed call in one transaction
//! near.transaction(contract_id)
//!     .state_init(state_init, NearToken::ZERO)
//!     .add_action(Counter::increment())
//!     .send().await?;
//!
//! // Compose calls from multiple contract standards
//! near.transaction("token.near")
//!     .add_action(StorageManagement::storage_deposit(deposit_args))
//!     .add_action(FungibleToken::ft_transfer_call(transfer_args))
//!     .send().await?;
//! ```
//!
//! # Serialization Formats
//!
//! By default, arguments are serialized as JSON. Use `#[near_kit::contract(borsh)]`
//! if the on-chain contract expects Borsh-encoded input:
//!
//! ```ignore
//! use borsh::BorshSerialize;
//!
//! #[derive(BorshSerialize)]
//! pub struct UploadArgs {
//!     pub data: Vec<u8>,
//! }
//!
//! #[near_kit::contract(borsh)]
//! pub trait DataStore {
//!     fn get_size(&self) -> u64;
//!
//!     #[call]
//!     fn upload(&mut self, args: UploadArgs);
//! }
//! ```
//!
//! You can also override the format per-method with `#[borsh]` or `#[json]`:
//!
//! ```ignore
//! #[near_kit::contract]  // default: JSON
//! pub trait MixedContract {
//!     fn get_status(&self) -> String;       // JSON (default)
//!
//!     #[call]
//!     fn set_config(&mut self, args: Config);  // JSON (default)
//!
//!     #[call]
//!     #[borsh]
//!     fn upload_data(&mut self, args: Data);   // Borsh (override)
//! }
//! ```

use crate::client::Near;
use crate::types::AccountId;

/// Marker trait for typed contract interfaces.
///
/// This trait is automatically implemented by the `#[near_kit::contract]` macro
/// for each contract interface you define. It provides the associated `Client` type
/// that is used by [`Near::contract`](crate::Near::contract).
///
/// # Example
///
/// Given a contract definition:
///
/// ```ignore
/// #[near_kit::contract]
/// pub trait MyContract {
///     fn get_value(&self) -> u64;
///     #[call]
///     fn set_value(&mut self, args: SetArgs);
/// }
/// ```
///
/// The macro generates a unit struct `MyContract` with composable
/// `FunctionCall` constructors, a `MyContractClient` for the simple case,
/// and this implementation:
///
/// ```ignore
/// impl Contract for MyContract {
///     type Client = MyContractClient;
/// }
/// ```
pub trait Contract {
    /// The generated client type for this contract interface.
    type Client: ContractClient;
}

/// Trait for contract client constructors.
///
/// This trait is implemented by the generated client structs to enable
/// construction via [`Near::contract`](crate::Near::contract).
pub trait ContractClient: Sized {
    /// Create a new contract client.
    fn new(near: Near, contract_id: AccountId) -> Self;
}
