//! Test: `clone` would shadow `Clone::clone` on the generated client.

use near_kit::*;

#[near_kit::contract]
pub trait Cloner {
    fn clone(&self) -> u64;
}

fn main() {}
