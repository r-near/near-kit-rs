//! Test: an argument must be named, or it can't be passed on.

use near_kit::*;

#[derive(serde::Serialize)]
pub struct Args {
    pub value: u64,
}

#[near_kit::contract]
pub trait Wildcard {
    #[call]
    fn set(&mut self, _: Args);
}

fn main() {}
