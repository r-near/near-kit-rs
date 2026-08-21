//! Test: the standalone #[json] marker rejects options

use near_kit::json;

#[json(pretty)]
fn get_value() {}

fn main() {}
