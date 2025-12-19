/// Minimal platform shim used by vendored code. The real host can register
/// platform delegates (enable/disable raw mode, size) via `set_tty_delegates`.
pub mod platform {
    use std::io;

    // Delegates removed: the shim now calls the host `bridges::tty` helpers directly.
    // Small convenience wrappers used by code that expects crossterm-like API.
    pub fn enable_raw_mode() -> io::Result<()> {
        crate::bridges::tty::enable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn disable_raw_mode() -> io::Result<()> {
        crate::bridges::tty::disable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn size() -> io::Result<(u16, u16)> {
        crate::bridges::tty::size().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
