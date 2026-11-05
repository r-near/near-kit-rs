//! Test: `client` (the `Contract` constructor on the marker type),
//! `contract_id`, `backend` and `into_parts` (the generated client's
//! accessors) and `clone` (it would shadow `Clone::clone`) are reserved, raw
//! identifiers included.

#[near_kit::contract]
pub trait Factory {
    #[call]
    fn client(&mut self);
}

#[near_kit::contract]
pub trait RawFactory {
    #[call]
    fn r#client(&mut self);
}

#[near_kit::contract]
pub trait Registry {
    fn contract_id(&self) -> String;
}

#[near_kit::contract]
pub trait Relay {
    fn backend(&self) -> String;
}

#[near_kit::contract]
pub trait Splitter {
    #[call]
    fn r#into_parts(&mut self);
}

#[near_kit::contract]
pub trait Cloner {
    fn clone(&self) -> u64;
}

fn main() {}
