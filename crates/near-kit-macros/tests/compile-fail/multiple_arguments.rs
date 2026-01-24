//! Test: multiple arguments should fail (must use struct)

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    fn add(&self, a: u64, b: u64) -> u64;
}

fn main() {}
