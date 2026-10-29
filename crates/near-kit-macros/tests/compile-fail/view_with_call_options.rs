//! Test: a view (&self) can't set gas or deposit

#[near_kit::contract]
pub trait BadContract {
    #[call(deposit = near_kit::NearToken::from_yoctonear(1))]
    fn get_value(&self) -> u64;
}

fn main() {}
