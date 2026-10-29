//! Test: unsafe, auto, generic, `where`-bounded and supertrait-bounded traits
//! are rejected, each at the offending part.

#[near_kit::contract]
pub unsafe trait Unsafe {
    fn get(&self) -> u64;
}

#[near_kit::contract]
pub auto trait Auto {}

#[near_kit::contract]
pub trait Generic<T> {
    fn get(&self) -> u64;
}

#[near_kit::contract]
pub trait Bounded
where
    Self: Sized,
{
    fn get(&self) -> u64;
}

#[near_kit::contract]
pub trait Extends: Clone {
    fn get(&self) -> u64;
}

fn main() {}
