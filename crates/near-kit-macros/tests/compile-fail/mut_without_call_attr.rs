//! Test: &mut self method without #[call] attribute should fail

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    fn mutate(&mut self);
}

fn main() {}
