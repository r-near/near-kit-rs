//! Test: near-sdk's `#[deny_unknown_arguments]` is rejected (it used to be
//! dropped)

#[near_kit::contract]
pub trait Registry {
    #[deny_unknown_arguments]
    fn lookup(&self, name: String) -> Option<String>;
}

fn main() {}
