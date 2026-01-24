//! Test: non-reference receiver (self instead of &self) should fail

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    fn consume(self) -> u64;
}

fn main() {}
