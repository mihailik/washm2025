mod command;
#[cfg(feature = "ion")]
mod ion_bridge;
mod uutils_bridge;
mod wasm_async;

use std::slice;
use std::str;

/// The main entrypoint for the Host to submit a command line.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers from the Host.
/// The Host must ensure `ptr` points to valid memory of `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn axy_submit(ptr: *const u8, len: usize) -> i32 {
    let slice = slice::from_raw_parts(ptr, len);
    match str::from_utf8(slice) {
        Ok(cmd) => {
            println!("Reactor received: {}", cmd);
            // TODO: Parse and dispatch command
            0
        }
        Err(_) => {
            eprintln!("axy_submit: invalid utf8");
            1
        }
    }
}

/// Standard WASI Reactor initialization.
#[no_mangle]
pub extern "C" fn _initialize() {
    // Initialization logic here (e.g. setting up env vars, logging)
}

// Re-export guest_alloc and _continue from wasm_async for the Host
pub use wasm_async::guest_alloc;
pub use wasm_async::_continue;
