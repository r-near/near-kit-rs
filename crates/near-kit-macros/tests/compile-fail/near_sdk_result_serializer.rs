//! Test: near-sdk's `#[result_serializer(..)]` is rejected (it used to be
//! dropped, so the result was decoded as JSON)

#[near_kit::contract]
pub trait Counter {
    #[result_serializer(borsh)]
    fn get(&self) -> u64;
}

fn main() {}
