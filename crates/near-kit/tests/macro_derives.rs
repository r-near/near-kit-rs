//! The `#[near_kit::contract]` macro must derive Debug + Clone on generated clients.
use near_kit::contract;

#[contract]
pub trait Counter {
    fn get_count(&self) -> u64;
    #[call]
    fn increment(&mut self);
}

fn _assert_debug_clone<T: std::fmt::Debug + Clone>() {}

#[test]
fn generated_client_is_debug_and_clone() {
    _assert_debug_clone::<CounterClient>();
    _assert_debug_clone::<Counter>();
}
