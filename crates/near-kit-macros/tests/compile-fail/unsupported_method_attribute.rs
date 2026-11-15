//! Test: attributes the macro neither reads nor copies are rejected on a
//! method (they used to be dropped), generic ones included

#[near_kit::contract]
pub trait Deprecated {
    #[deprecated = "use get_v2"]
    fn get(&self) -> u64;
}

#[near_kit::contract]
pub trait Allowed {
    #[allow(clippy::too_many_arguments)]
    #[call]
    fn set(&mut self, value: u64);
}

#[near_kit::contract]
pub trait MustUse {
    #[must_use]
    fn get(&self) -> u64;
}

#[near_kit::contract]
pub trait PathAttribute {
    #[rustfmt::skip]
    fn get(&self) -> u64;
}

fn main() {}
