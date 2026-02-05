#[no_mangle]
pub extern "C" fn dropper() {
    let x = Box::new(42);
    std::mem::drop(x);
}
