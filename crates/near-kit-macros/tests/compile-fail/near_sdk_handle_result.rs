//! Test: near-sdk's `#[handle_result]` is rejected (it used to be dropped),
//! before the call's return type is reported

#[near_kit::contract]
pub trait Registry {
    #[handle_result]
    #[call]
    fn register(&mut self, name: String) -> Result<(), String>;
}

fn main() {}
