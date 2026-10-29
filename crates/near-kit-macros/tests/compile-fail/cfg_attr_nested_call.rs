//! Test: `#[call]` inside a nested `#[cfg_attr]` is rejected too.

#[near_kit::contract]
pub trait Conditional {
    #[cfg_attr(all(), cfg_attr(all(), call))]
    fn set(&mut self);
}

fn main() {}
