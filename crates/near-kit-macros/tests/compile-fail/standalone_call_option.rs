//! Test: the standalone marker macro also rejects options

use near_kit::call;

#[call(payable)]
fn do_something() {}

fn main() {}
