//! Test: a #[call] option can't read the method's argument; defaults are
//! per method

use near_kit::Gas;

#[near_kit::contract]
pub trait BadContract {
    #[call(gas = Gas::from_tgas(tgas))]
    fn do_something(&mut self, tgas: u64);
}

fn main() {}
