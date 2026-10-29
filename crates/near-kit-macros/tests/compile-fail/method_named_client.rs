//! Test: `client` is the `Contract` constructor on the marker type.

use near_kit::*;

#[near_kit::contract]
pub trait Factory {
    #[call]
    fn client(&mut self);
}

fn main() {}
