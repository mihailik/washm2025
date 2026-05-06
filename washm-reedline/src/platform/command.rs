//! Terminal command primitives (cursor moves, color changes, clears, …).
//!
//! Phase C-1: native re-exports of [`crossterm`] command types when the
//! `crossterm-backend` feature is enabled. Phase C-2 will fill in the
//! `inner` module with reedline-owned ANSI emitters so the painter can
//! compile on `wasm32-*` targets without `crossterm`.

#[cfg(feature = "crossterm-backend")]
pub use crossterm::{
    cursor::{Hide, MoveTo, MoveUp, RestorePosition, SavePosition, Show},
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
    QueueableCommand,
};

/// Query the current cursor position on the underlying terminal.
///
/// Native builds delegate to [`crossterm::cursor::position`]. Builds
/// without the `crossterm-backend` feature have no real terminal to
/// query and report `(0, 0)`; hosts driving reedline through a virtual
/// terminal (e.g. xterm.js via WASM) should track cursor state
/// themselves and not rely on this helper.
#[cfg(feature = "crossterm-backend")]
pub fn cursor_position() -> std::io::Result<(u16, u16)> {
    crossterm::cursor::position()
}

#[cfg(not(feature = "crossterm-backend"))]
pub fn cursor_position() -> std::io::Result<(u16, u16)> {
    Ok((0, 0))
}

#[cfg(not(feature = "crossterm-backend"))]
mod inner;

#[cfg(not(feature = "crossterm-backend"))]
pub use inner::{
    Attribute, Clear, ClearType, Hide, MoveTo, MoveUp, Print, QueueableCommand, ResetColor,
    RestorePosition, SavePosition, SetAttribute, SetForegroundColor, Show,
};
