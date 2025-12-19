use std::io::Write;
use std::os::wasi::io::FromRawFd;

/// JS payload that implements the host protocol.
const PROTOCOL_JS: &str = include_str!("protocol.js");

/// Reserved FD for protocol bootstrap.
const BOOTSTRAP_FD: i32 = 100;

/// Bootstrap the host protocol by writing the JS payload to the reserved FD.
pub fn bootstrap_host_protocol() {
    unsafe {
        let js_bytes = PROTOCOL_JS.as_bytes();
        
        // Use libc::write directly to avoid any std::fs overhead or ownership issues
        let _ = libc::write(BOOTSTRAP_FD, js_bytes.as_ptr() as *const libc::c_void, js_bytes.len());
        
        // Also close it to trigger the host-side eval
        let _ = libc::close(BOOTSTRAP_FD);
    }
}
