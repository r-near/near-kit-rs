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

/// More than seven argument-taking methods: the generated type-mention helpers
/// must not add up to a function with too many arguments.
#[near_kit::contract]
pub trait ManyMethods {
    /// View 1.
    fn v1(&self, args: Args) -> u64;
    /// View 2.
    fn v2(&self, args: Args) -> u64;
    /// View 3.
    fn v3(&self, args: Args) -> u64;
    /// View 4.
    fn v4(&self, args: Args) -> u64;
    /// Call 1.
    #[call]
    fn c1(&mut self, args: Args);
    /// Call 2.
    #[call]
    fn c2(&mut self, args: Args);
    /// Call 3.
    #[call]
    fn c3(&mut self, args: Args);
    /// Call 4.
    #[call]
    fn c4(&mut self, args: Args);
}

#[test]
fn many_methods_expand() {
    let call = ManyMethods::c1(Args { value: 1 });
    assert!(call.into_action().is_ok());
}
