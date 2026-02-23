#[no_mangle]
pub extern "C" fn struct_get_set(ptr: *mut i32) {
    unsafe {
        let val = *ptr;
        *ptr = val + 1;
    }
}
