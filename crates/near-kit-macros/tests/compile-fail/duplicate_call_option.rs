//! Test: each #[call] option may only be given once

#[near_kit::contract]
pub trait BadContract {
    #[call(gas = near_kit::Gas::from_tgas(100), gas = near_kit::Gas::from_tgas(200))]
    fn do_something(&mut self);
}

fn main() {}
