//! Test: near-sdk's `#[init]` is rejected (it used to be dropped), before
//! the missing receiver is reported, with or without options

#[near_kit::contract]
pub trait Token {
    #[init]
    fn new(owner_id: String) -> Self;
}

#[near_kit::contract]
pub trait Migrated {
    #[init(ignore_state)]
    #[call]
    fn r#migrate(&mut self);
}

fn main() {}
