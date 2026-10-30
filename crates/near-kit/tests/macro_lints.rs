//! `#[near_kit::contract]` output must stay clean under strict downstream
//! lint configurations. CI runs clippy over this file with `-D warnings`.
#![cfg(feature = "contracts")]
#![deny(clippy::all, clippy::pedantic)]

use serde::Serialize;

/// Shared argument type for the methods below.
#[derive(Serialize)]
pub struct Args {
    /// A value.
    pub value: u64,
}

/// Several argument-taking views and calls, so clippy sees every generated
/// method shape.
#[near_kit::contract]
pub trait ManyMethods {
    /// View 1.
    fn v1(&self, #[args] args: Args) -> u64;
    /// View 2.
    fn v2(&self, #[args] args: Args) -> u64;
    /// View 3.
    fn v3(&self, value: u64) -> u64;
    /// View 4.
    fn v4(&self, value: u64) -> u64;
    /// Call 1.
    #[call]
    fn c1(&mut self, #[args] args: Args);
    /// Call 2.
    #[call]
    fn c2(&mut self, #[args] args: Args);
    /// Call 3.
    #[call]
    fn c3(&mut self, value: u64);
    /// Call 4.
    #[call]
    fn c4(&mut self, value: u64);
    /// A view with several parameters, borrowed and owned.
    fn v5(&self, account_id: &str, limit: Option<u32>, args: Args) -> u64;
    /// A call with as many parameters as near-sdk contracts declare.
    #[call]
    fn c5(&mut self, a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8);
    /// A borrowed parameter.
    #[call]
    fn c6(&mut self, account_id: &str);
}

/// Borsh methods with parameters.
#[near_kit::contract(borsh)]
pub trait BorshMethods {
    /// A view.
    fn v(&self, a: u64, b: Vec<u8>) -> u64;
    /// A call.
    #[call]
    fn c(&mut self, a: u64, b: Vec<u8>);
}

#[test]
fn many_methods_expand() {
    let call = ManyMethods::c1(Args { value: 1 });
    assert!(call.into_action().is_ok());
}
