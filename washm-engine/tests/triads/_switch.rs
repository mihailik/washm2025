#[no_mangle]
pub extern "C" fn sw(x: i32) -> i32 {
    match x {
        0 => 0,
        1 => 10,
        2 => 20,
        3 | 4 => 30,
        _ => 99,
    }
}
