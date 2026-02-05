#[no_mangle]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[no_mangle]
pub fn factorial(n: i32) -> i32 {
    let mut res = 1;
    let mut i = 1;
    while i <= n {
        res *= i;
        i += 1;
    }
    res
}
