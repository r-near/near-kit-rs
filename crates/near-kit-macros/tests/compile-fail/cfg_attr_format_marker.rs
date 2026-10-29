//! Test: a format marker behind `#[cfg_attr]` is rejected, not forwarded.

#[near_kit::contract]
pub trait Conditional {
    #[cfg_attr(all(), borsh)]
    fn get(&self) -> u64;
}

fn main() {}
