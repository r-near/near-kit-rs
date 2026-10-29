//! Test: #[call] may only be given once

#[near_kit::contract]
pub trait BadContract {
    #[call]
    #[call(gas = near_kit::Gas::from_tgas(100))]
    fn do_something(&mut self);
}

fn main() {}
