/// JS payload that implements the host protocol.
const PROTOCOL_JS: &str = include_str!("protocol.js");
const EVAL_JS: &str = include_str!("bridges/eval.js");
const HOST_JS: &str = include_str!("bridges/host.js");
const PROCESS_JS: &str = include_str!("bridges/process.js");
const TTY_JS: &str = include_str!("bridges/tty.js");
const TIMER_JS: &str = include_str!("bridges/timer.js");

/// Reserved FD for protocol bootstrap.
const BOOTSTRAP_FD: i32 = 20251219;

/// Bootstrap the host protocol by writing the JS payload to the reserved FD.
pub fn bootstrap_host_protocol() {
    unsafe {
        let mut payload = String::from(PROTOCOL_JS);
        payload.push_str(EVAL_JS);
        payload.push_str(HOST_JS);
        payload.push_str(PROCESS_JS);
        payload.push_str(TTY_JS);
        payload.push_str(TIMER_JS);
        
        let js_bytes = payload.as_bytes();
        
        // Use libc::write directly to avoid any std::fs overhead or ownership issues
        let _ = libc::write(BOOTSTRAP_FD, js_bytes.as_ptr() as *const libc::c_void, js_bytes.len());
        
        // Also close it to trigger the host-side eval
        let _ = libc::close(BOOTSTRAP_FD);
    }
}
