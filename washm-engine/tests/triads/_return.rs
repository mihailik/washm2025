#[no_mangle]
pub extern "C" fn ret(a: i32, b: i32) -> i32 {
    a + b
}
