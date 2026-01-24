//! Compile-fail tests for the near-kit-macros crate.
//!
//! These tests verify that the macro produces the expected compile-time errors
//! for invalid usage.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/*.rs");
}
