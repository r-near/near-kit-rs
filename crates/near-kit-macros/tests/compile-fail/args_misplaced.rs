//! Test: `#[args]` goes on the parameter (on the method it would be
//! dropped), and takes no options

#[derive(serde::Serialize)]
pub struct Args {
    pub value: u64,
}

#[near_kit::contract]
pub trait OnTheMethod {
    #[args]
    fn get(&self, args: Args) -> u64;
}

#[near_kit::contract]
pub trait WithOptions {
    fn get(&self, #[args(with = Args::new)] args: Args) -> u64;
}

fn main() {}
