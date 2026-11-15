//! Test: near-sdk's `#[payable]` is rejected (it used to be dropped),
//! before the missing `#[call]` is reported

#[near_kit::contract]
pub trait Token {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: String, amount: u64);
}

fn main() {}
