//! Test: methods with type or lifetime parameters are rejected.

#[near_kit::contract]
pub trait Generic {
    fn get<T: serde::Serialize>(&self, args: T) -> u64;
}

#[near_kit::contract]
pub trait Lifetime {
    fn get<'a>(&self, key: &'a str) -> u64;
}

fn main() {}
