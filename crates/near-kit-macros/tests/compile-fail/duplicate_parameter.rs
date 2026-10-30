//! Test: two parameters can't share a name

#[near_kit::contract]
pub trait BadContract {
    fn get(&self, account_id: String, account_id: String) -> u64;
}

fn main() {}
