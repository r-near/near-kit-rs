//! Test: #[json] options are rejected inside #[contract]

#[near_kit::contract]
pub trait BadContract {
    #[json(pretty)]
    fn get_value(&self) -> u64;
}

fn main() {}
