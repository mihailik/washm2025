use std::io::Result;

/// Defines the control plane for the terminal, such as entering/exiting raw mode
/// or managing signals.
pub trait TerminalControl: Send {
    /// Enter raw mode and perform any terminal-specific initialization.
    fn enter_raw_mode(&mut self) -> Result<()>;

    /// Exit raw mode and restore standard terminal settings.
    fn exit_raw_mode(&mut self) -> Result<()>;

    /// Query the current terminal window size (cols, rows).
    fn terminal_size(&self) -> (u16, u16);

    /// Get current cursor position (column, row).
    fn cursor_position(&self) -> (u16, u16);
}

/// A standard implementation using `crossterm`.
///
/// Only available when the `crossterm-backend` feature is enabled. On
/// targets without that feature (e.g. `wasm32-*` web builds) use
/// [`ExternalTerminalControl`] and have the JavaScript host drive the
/// terminal capabilities.
#[cfg(feature = "crossterm-backend")]
pub struct CrosstermTerminalControl;

#[cfg(feature = "crossterm-backend")]
impl TerminalControl for CrosstermTerminalControl {
    fn enter_raw_mode(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()
    }

    fn exit_raw_mode(&mut self) -> Result<()> {
        crossterm::terminal::disable_raw_mode()
    }

    fn terminal_size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }

    fn cursor_position(&self) -> (u16, u16) {
        crossterm::cursor::position().unwrap_or((0, 0))
    }
}

/// A virtual implementation for WASM/Web environments.
pub struct ExternalTerminalControl {
    width: u16,
    height: u16,
}

impl ExternalTerminalControl {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub fn update_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }
}

impl TerminalControl for ExternalTerminalControl {
    fn enter_raw_mode(&mut self) -> Result<()> {
        // In a web terminal, "raw mode" is often a no-op or a CSS class change
        // handled in the JS host.
        Ok(())
    }

    fn exit_raw_mode(&mut self) -> Result<()> {
        Ok(())
    }

    fn terminal_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn cursor_position(&self) -> (u16, u16) {
        (0, 0)
    }
}
