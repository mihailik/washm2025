//! Cursor styling primitives.
//!
//! Phase A: native re-export of [`crossterm::cursor::SetCursorStyle`]
//! when the `crossterm-backend` feature is enabled. Phase C will
//! provide a reedline-owned equivalent for non-`crossterm` targets.

#[cfg(feature = "crossterm-backend")]
pub use crossterm::cursor::SetCursorStyle;

#[cfg(not(feature = "crossterm-backend"))]
mod inner;

#[cfg(not(feature = "crossterm-backend"))]
pub use inner::SetCursorStyle;
