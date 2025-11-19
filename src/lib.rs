mod command;
#[cfg(feature = "ion")]
mod ion_bridge;
mod uutils_bridge;
mod wasm_async;
mod async_test;
mod jobs;

use std::slice;
use std::str;
use std::ffi::OsString;

/// The main entrypoint for the Host to submit a command line.
#[no_mangle]
pub unsafe extern "C" fn axy_submit(ptr: *const u8, len: usize) -> i32 {
    let slice = slice::from_raw_parts(ptr, len);
    match str::from_utf8(slice) {
        Ok(cmd_line) => {
            let args: Vec<String> = cmd_line.split_whitespace().map(|s| s.to_string()).collect();
            if args.is_empty() {
                return 0;
            }

            match args[0].as_str() {
                "ls" => {
                    let args_os: Vec<OsString> = args.iter().map(|s| OsString::from(s)).collect();
                    // uu_ls::uumain returns i32 (exit code) due to #[uucore::main] macro
                    uu_ls::uumain(args_os.into_iter())
                }
                "echo" => {
                    use std::io::{Write, stdout};
                    let mut out = stdout();
                    if args.len() > 1 {
                        let _ = writeln!(&mut out, "{}", args[1..].join(" "));
                    } else {
                        let _ = writeln!(&mut out);
                    }
                    let _ = out.flush();
                    0
                }
                "test-async" => {
                    unsafe { crate::async_test::test_async_callback() }
                }
                _ => {
                    eprintln!("axy: command not found: {}", args[0]);
                    127
                }
            }
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
    // Initialization logic here
}

// Re-export guest_alloc and _continue from wasm_async for the Host
pub use wasm_async::guest_alloc;
pub use wasm_async::_continue;

