//! Compile tests for the near-kit-macros crate.
//!
//! These tests verify that the macro produces the expected compile-time errors
//! for invalid usage, and that valid usage compiles correctly.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/*.rs");
}

#[test]
fn compile_pass() {
    let t = trybuild::TestCases::new();
    t.pass("tests/compile-pass/*.rs");
}
