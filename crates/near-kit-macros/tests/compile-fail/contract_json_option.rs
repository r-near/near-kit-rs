//! Test: #[json] options are rejected inside #[contract]

#[allow(unused_imports)]
use near_kit::json;

#[near_kit::contract]
pub trait BadContract {
    #[json(pretty)]
    fn get_value(&self) -> u64;
}

fn main() {}
