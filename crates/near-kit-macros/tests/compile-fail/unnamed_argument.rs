//! Test: an argument must be a plain name (not `_` or a destructuring
//! pattern), or it can't be passed on.

#[derive(serde::Serialize)]
pub struct Args {
    pub value: u64,
}

#[near_kit::contract]
pub trait Wildcard {
    #[call]
    fn set(&mut self, _: Args);
}

#[near_kit::contract]
pub trait Destructured {
    fn get(&self, (a, b): (u64, u64)) -> u64;
}

fn main() {}
