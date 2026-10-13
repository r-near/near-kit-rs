//! Without `rpc`, the `Near`-backed client and the `Contract` impl are not
//! generated.

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
