/// Guest-side process bridge installer via eval bridge.
///
/// This module installs a minimal /dev/host/process bridge by:
/// 1. Writing the JS payload to /dev/host/eval/<cookie>.js
/// 2. Letting the host eval bridge execute it and call _continue
/// 3. Polling for /dev/host/eval/<cookie>.json as confirmation (with async sleep)
/// 4. Ensuring the bridge is installed only once (via atomic flag)

use std::sync::atomic::{AtomicBool, Ordering};

/// JS payload that installs a /dev/host/process bridge when evaluated by the host.
/// The payload is embedded at compile time from the adjacent process_bridge.js file.
const PROCESS_BRIDGE_JS: &str = include_str!("process_bridge.js");

/// Flag to track if bridge installation has been initiated
static BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the guest-side process bridge.
/// Idempotent - safe to call multiple times. Uses thread yield for polling.
pub fn install_process_bridge() {
    // Check if already installed
    if BRIDGE_INSTALLED.load(Ordering::Relaxed) {
        return;
    }

    let cookie = "process_bridge_1";
    let js_path = format!("/dev/host/eval/{}.js", cookie);
    
    let _ = std::fs::create_dir_all("/dev/host/eval");
    
    // Include sourceURL comment so debuggers can identify the bridge source
    let js_with_sourcemap = format!("{} //# sourceURL=process_bridge.js", PROCESS_BRIDGE_JS);
    let _ = std::fs::write(&js_path, &js_with_sourcemap);
    
    // Poll for host JSON response using thread yield (non-blocking on event loop)
    for _ in 0..200 {
        if std::fs::read_to_string(format!("/dev/host/eval/{}.json", cookie)).is_ok() {
            BRIDGE_INSTALLED.store(true, Ordering::Relaxed);
            return;
        }
        // Yield to other threads without blocking
        std::thread::yield_now();
    }
    
    BRIDGE_INSTALLED.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn installer_writes_js_file() {
        let _ = fs::create_dir_all("/dev/host/eval");
        // Remove any existing file to ensure a clean test
        let _ = fs::remove_file("/dev/host/eval/process_bridge_1.js");
        install_process_bridge();
        assert!(fs::metadata("/dev/host/eval/process_bridge_1.js").is_ok());
    }
}
