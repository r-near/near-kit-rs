//! Test: `#[args]` makes its parameter the whole argument object, so it must
//! be the method's only parameter

#[derive(serde::Serialize)]
pub struct Args {
    pub value: u64,
}

#[near_kit::contract]
pub trait BadContract {
    fn get(&self, #[args] args: Args, limit: u32) -> u64;
}

fn main() {}
