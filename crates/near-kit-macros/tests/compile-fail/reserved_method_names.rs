//! Test: `client` (the `Contract` constructor on the marker type),
//! `contract_id` (the generated client's accessor) and `clone` (it would
//! shadow `Clone::clone`) are reserved, raw identifiers included.

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
pub trait Cloner {
    fn clone(&self) -> u64;
}

fn main() {}
