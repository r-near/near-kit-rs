//! Test: #[call] does not accept options

#[near_kit::contract]
pub trait BadContract {
    #[call]
    #[call(payable)]
    fn do_something(&mut self);
}

fn main() {}
