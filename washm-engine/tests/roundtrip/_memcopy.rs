#[no_mangle]
pub extern "C" fn memcopy(src: *const u8, dst: *mut u8, count: usize) {
    unsafe {
        std::ptr::copy_nonoverlapping(src, dst, count);
    }
}
