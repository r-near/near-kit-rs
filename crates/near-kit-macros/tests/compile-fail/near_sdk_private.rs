//! Test: near-sdk's `#[private]` is rejected (it used to be dropped)

#[near_kit::contract]
pub trait Token {
    #[private]
    #[call]
    fn ft_resolve_transfer(&mut self, sender_id: String, amount: u64);
}

fn main() {}
