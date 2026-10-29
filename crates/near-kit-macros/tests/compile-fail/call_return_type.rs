//! Test: a `#[call]` method's return type would be ignored, so it is rejected.

use near_kit::*;

#[near_kit::contract]
pub trait Escrow {
    #[call]
    fn close(&mut self) -> bool;
}

fn main() {}
