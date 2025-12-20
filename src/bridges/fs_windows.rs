pub const BRIDGE_JS: &str = include_str!("fs_windows.js");

pub fn ensure_bridge() {
    let js = format!("{}\n//# sourceURL=bridges/fs_windows.js\n", BRIDGE_JS);
    crate::bootstrap::write_js(&js);
}


