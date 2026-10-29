//! Test: a destructuring argument pattern is rejected.

use near_kit::*;

#[near_kit::contract]
pub trait Destructured {
    fn get(&self, (a, b): (u64, u64)) -> u64;
}

fn main() {}
