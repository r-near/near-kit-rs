//! Test: a format chosen by `cfg` is two declarations, each under its own
//! `#[cfg]` (a format marker behind `#[cfg_attr]` is rejected).

use near_kit::*;

#[near_kit::contract]
pub trait Conditional {
    #[cfg(all())]
    #[borsh]
    fn get(&self) -> u64;

    #[cfg(not(all()))]
    fn get(&self) -> u64;
}

fn main() {
    let near = Near::testnet().build();
    let _view = near.contract::<Conditional>(account!("conditional.testnet")).get();
}
