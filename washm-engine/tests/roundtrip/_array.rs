#[no_mangle]
pub extern "C" fn arr(n: i32) -> i32 {
    let mut v = [0i32; 10];
    let idx = (n as usize) % 10;
    v[idx] = 7;
    v[idx]
}
