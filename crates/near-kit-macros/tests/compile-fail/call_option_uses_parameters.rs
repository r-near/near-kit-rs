//! Test: a #[call] option can't read any of the method's parameters

use near_kit::Gas;

#[near_kit::contract]
pub trait BadContract {
    #[call(gas = Gas::from_tgas(tgas))]
    fn do_something(&mut self, account_id: String, tgas: u64);
}

fn main() {}
