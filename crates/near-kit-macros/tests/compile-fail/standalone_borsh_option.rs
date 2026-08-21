//! Test: the standalone #[borsh] marker rejects options

use near_kit::borsh;

#[borsh(compact)]
fn get_value() {}

fn main() {}
