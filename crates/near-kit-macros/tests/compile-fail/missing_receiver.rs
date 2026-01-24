//! Test: missing receiver (no self) should fail

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    fn static_method() -> u64;
}

fn main() {}
