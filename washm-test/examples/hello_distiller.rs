#[allow(clippy::eq_op)]
fn simple_test() {
    assert_eq!(2 + 2, 4);
}

fn main() {
    // Access the symbol to ensure it's linked
    unsafe {
        let _ = washm_test_guest::__WASHM_COV_PTR;
    }
    simple_test();
}
