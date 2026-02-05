#[no_mangle]
pub extern "C" fn struct_get_set(a: i32) -> i32 {
    struct S { f: i32 }
    let mut s = S { f: a };
    s.f = s.f + 1;
    s.f
}
