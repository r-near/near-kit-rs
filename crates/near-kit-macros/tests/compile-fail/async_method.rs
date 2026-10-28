//! Test: `async fn` is rejected; the generated methods return futures already.

use near_kit::*;

#[near_kit::contract]
pub trait Async {
    async fn get(&self) -> u64;
}

fn main() {}
