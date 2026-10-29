//! Test: a raw identifier doesn't get around a reserved name.

use near_kit::*;

#[near_kit::contract]
pub trait Factory {
    #[call]
    fn r#client(&mut self);
}

fn main() {}
