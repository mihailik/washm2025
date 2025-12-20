pub const BRIDGE_JS: &str = include_str!("fs_windows.js");

pub fn ensure_bridge() {
    crate::bootstrap::write_js(BRIDGE_JS);
    crate::bootstrap::write_js("\n//# sourceURL=bridges/fs_windows.js\n");
}

