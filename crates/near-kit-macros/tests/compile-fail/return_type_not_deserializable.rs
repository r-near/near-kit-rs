//! Test: a view's return type that can't be decoded is reported at the type

pub struct NotDeserializable;

#[near_kit::contract]
pub trait BadContract {
    fn get(&self) -> NotDeserializable;

    #[borsh]
    fn find(&self, key: u8) -> NotDeserializable;
}

fn main() {}
