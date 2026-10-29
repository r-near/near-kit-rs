//! Test: `contract_id` is the generated client's accessor.

use near_kit::*;

#[near_kit::contract]
pub trait Registry {
    fn contract_id(&self) -> String;
}

fn main() {}
