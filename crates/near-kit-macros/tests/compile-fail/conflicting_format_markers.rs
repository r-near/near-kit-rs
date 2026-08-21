//! Test: #[json] and #[borsh] cannot be combined on one method

#[allow(unused_imports)]
use near_kit::{borsh, json};

#[near_kit::contract]
pub trait BadContract {
    #[json]
    #[borsh]
    fn get_value(&self) -> u64;
}

fn main() {}
