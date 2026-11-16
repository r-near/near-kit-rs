//! Test: an `#[args]` parameter that can't be encoded is reported at the
//! parameter, in views and calls alike

pub struct NotSerializable;

#[near_kit::contract]
pub trait BadContract {
    fn get(&self, #[args] args: NotSerializable) -> u64;

    #[borsh]
    fn find(&self, #[args] args: NotSerializable) -> u64;

    #[call]
    fn set(&mut self, #[args] args: NotSerializable);

    #[call]
    #[borsh]
    fn store(&mut self, #[args] args: NotSerializable);
}

fn main() {}
