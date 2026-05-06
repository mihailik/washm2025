//! Keyboard, mouse, and terminal events.
//!
//! Phase A: native re-exports of [`crossterm::event`] types when the
//! `crossterm-backend` feature is enabled.
//!
//! Phase C will populate the `inner` module below with reedline-owned
//! definitions that are compiled when the backend is disabled (e.g. on
//! `wasm32-*` targets).

#[cfg(feature = "crossterm-backend")]
pub use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
    ModifierKeyCode, MouseButton, MouseEvent, MouseEventKind,
};

#[cfg(not(feature = "crossterm-backend"))]
mod inner;

#[cfg(not(feature = "crossterm-backend"))]
pub use inner::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
    ModifierKeyCode, MouseButton, MouseEvent, MouseEventKind,
};
