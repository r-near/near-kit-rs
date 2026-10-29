//! Test: `gas` must be a `Gas` and `deposit` a `NearToken`

use near_kit::{Gas, NearToken};

#[near_kit::contract]
pub trait BadContract {
    #[call(gas = NearToken::from_yoctonear(1), deposit = Gas::from_tgas(100))]
    fn do_something(&mut self);
}

fn main() {}
