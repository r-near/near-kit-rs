//! Test: methods with lifetime parameters are rejected.

use near_kit::*;

#[near_kit::contract]
pub trait Lifetime {
    fn get<'a>(&self, key: &'a str) -> u64;
}

fn main() {}
