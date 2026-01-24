//! Test: unknown call option in #[call(lazy)] should fail

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    #[call(lazy)]
    fn do_something(&mut self);
}

fn main() {}
