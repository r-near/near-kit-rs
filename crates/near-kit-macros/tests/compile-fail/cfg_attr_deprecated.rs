//! Test: `#[cfg_attr]` on a method reaches the generated items.

#![deny(deprecated)]

use near_kit::transaction::FunctionCall;

#[near_kit::contract]
pub trait Configured {
    #[cfg_attr(all(), deprecated = "use new_call")]
    #[call]
    fn old_call(&mut self);
}

fn main() {
    let _call: FunctionCall = Configured::old_call();
}
