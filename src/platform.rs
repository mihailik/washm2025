use std::cell::Cell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Node,
    Browser,
}

thread_local! {
    static CACHED_PLATFORM: Cell<Option<Platform>> = Cell::new(None);
}

/// Set the platform (called from JavaScript during bootstrap)
pub fn set_platform(is_browser: bool) {
    let platform = if is_browser {
        Platform::Browser
    } else {
        Platform::Node
    };
    CACHED_PLATFORM.with(|cell| cell.set(Some(platform)));
}

/// Detect the platform - returns cached value or defaults to Node
pub fn detect_platform() -> Platform {
    CACHED_PLATFORM.with(|cell| cell.get()).unwrap_or(Platform::Node)
}

/// Check if we're running in a browser environment
pub fn is_browser() -> bool {
    detect_platform() == Platform::Browser
}

/// Check if we're running in a Node.js environment
pub fn is_node() -> bool {
    detect_platform() == Platform::Node
}
