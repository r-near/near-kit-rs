//! Test: unknown format in #[near_kit::contract(xml)] should fail

use near_kit::*;

#[near_kit::contract(xml)]
pub trait BadContract {
    fn get_value(&self) -> u64;
}

fn main() {}
