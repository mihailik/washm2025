#[no_mangle]
pub extern "C" fn tup(a: i32, b: i32) -> i32 {
    let t = (a, b);
    t.1
}
