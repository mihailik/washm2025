/// JS payload that implements the host protocol.
const PROTOCOL_JS: &str = include_str!("protocol.js");

/// Reserved FD for protocol bootstrap.
const BOOTSTRAP_FD: i32 = 20251219;

pub fn write_js(js: &str) {
    unsafe {
        let js_bytes = js.as_bytes();
        let _ = libc::write(BOOTSTRAP_FD, js_bytes.as_ptr() as *const libc::c_void, js_bytes.len());
    }
}

pub fn close_bootstrap() {
    unsafe {
        let _ = libc::close(BOOTSTRAP_FD);
    }
}

/// Bootstrap the host protocol by writing the JS payload to the reserved FD.
pub fn bootstrap_host_protocol() {
    write_js(PROTOCOL_JS);
    write_js("\n//# sourceURL=protocol.js\n");
}
