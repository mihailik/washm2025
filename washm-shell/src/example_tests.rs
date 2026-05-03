// Example tests for POC
#![cfg(feature = "wasm-test")]

use crate::test_harness::register_test;

// Simple sync test
fn test_basic_math() {
    assert_eq!(2 + 2, 4);
    assert_eq!(10 * 5, 50);
}

// Another sync test
fn test_string_ops() {
    let s = "hello".to_uppercase();
    assert_eq!(s, "HELLO");
}

fn test_coverage_handshake() {
    let dummy_ptr = 0x12345678u32;
    crate::bridges::host::set_coverage_ptr(dummy_ptr)
        .expect("Failed to write to /dev/host/washm/coverage_ptr");
    let read_ptr = crate::bridges::host::get_coverage_ptr();
    assert_eq!(dummy_ptr, read_ptr, "Coverage pointer handshake failed");
    println!(
        "Coverage Handshake Successful: Shared PTR is 0x{:X}",
        read_ptr
    );
}

// Registration function called explicitly
pub fn register_example_tests() {
    register_test("test_basic_math", test_basic_math);
    register_test("test_string_ops", test_string_ops);
    register_test("test_coverage_handshake", test_coverage_handshake);
}
