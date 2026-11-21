#![cfg(feature = "derive")]

#[test]
fn compile_failures() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
}
