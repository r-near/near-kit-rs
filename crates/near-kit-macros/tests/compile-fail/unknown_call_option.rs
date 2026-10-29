//! Test: #[call] accepts only the `gas` and `deposit` options

#[near_kit::contract]
pub trait BadContract {
    #[call(payable)]
    fn do_something(&mut self);
}

fn main() {}
