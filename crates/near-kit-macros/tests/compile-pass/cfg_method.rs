//! Test: a method's `#[cfg]` and `#[cfg_attr]` apply to everything generated
//! for it, so a configured-out method may name items that don't exist.

use near_kit::transaction::FunctionCall;
use near_kit::*;

#[near_kit::contract]
pub trait Configured {
    #[cfg(any())]
    fn missing_view(&self, args: DoesNotExist) -> u64;

    #[cfg(any())]
    #[call]
    fn missing_call(&mut self, args: DoesNotExist);

    #[cfg(all())]
    fn present_view(&self) -> u64;

    #[cfg(all())]
    #[call]
    fn present_call(&mut self);

    #[cfg_attr(all(), deprecated = "use present_call")]
    #[call]
    fn old_call(&mut self);
}

fn main() {
    let near = Near::testnet().build();
    let client = near.contract::<Configured>(account!("configured.testnet"));
    let _view = client.present_view();
    let _call: FunctionCall = Configured::present_call();
    #[allow(deprecated)]
    let _old: FunctionCall = Configured::old_call();
}
