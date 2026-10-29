//! Test: a serialization format marker cannot be repeated

#[near_kit::contract]
pub trait BadContract {
    #[json]
    #[json]
    fn get_value(&self) -> u64;
}

fn main() {}
