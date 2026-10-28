//! Test: generic methods are rejected.

use near_kit::*;

#[near_kit::contract]
pub trait Generic {
    fn get<T: serde::Serialize>(&self, args: T) -> u64;
}

fn main() {}
