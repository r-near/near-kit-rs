//! Test: view method (&self) with #[call] attribute should fail

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    #[call]
    fn get_value(&self) -> u64;
}

fn main() {}
