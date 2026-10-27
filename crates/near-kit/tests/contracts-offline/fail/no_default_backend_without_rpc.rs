//! Without `rpc` there is no `Near`, so the generated client's backend has
//! no default and must be named.

#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

fn main() {
    let _ = Counter::increment();
    let _: Option<CounterClient> = None;
}
