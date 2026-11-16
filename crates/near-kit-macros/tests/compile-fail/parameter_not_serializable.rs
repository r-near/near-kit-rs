//! Test: a parameter that can't be encoded is reported at the parameter, once

pub struct NotSerializable;

#[near_kit::contract]
pub trait BadContract {
    fn get(&self, account_id: String, filter: NotSerializable) -> u64;

    #[borsh]
    fn find(&self, key: u8, value: NotSerializable) -> u64;

    fn both(&self, first: NotSerializable, second: NotSerializable) -> u64;

    #[call]
    fn set(&mut self, value: NotSerializable);

    #[call]
    #[borsh]
    fn store(&mut self, key: u8, value: NotSerializable);
}

fn main() {}
