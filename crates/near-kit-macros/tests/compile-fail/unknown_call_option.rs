//! Test: #[call] does not accept options

use near_kit::*;

#[near_kit::contract]
pub trait BadContract {
    #[call]
    #[call(payable)]
    fn do_something(&mut self);
}

fn main() {}
