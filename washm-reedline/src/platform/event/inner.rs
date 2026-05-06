//! Reedline-owned event type definitions used when the
//! `crossterm-backend` feature is disabled.
//!
//! **NOTE (Phase A scaffolding):** these definitions are not yet
//! populated. Phase C of the WASM publication plan will fill them in
//! with `crossterm`-API-compatible enums and structs so reedline
//! compiles for `wasm32-*` targets without pulling in `crossterm`.

compile_error!(
    "src/platform/event/inner.rs has not been populated yet (Phase C). \
     Build with the `crossterm-backend` feature enabled (default) for now."
);

// Placeholders to make rust-analyzer happier while editing. The
// `compile_error!` above will fire long before any of these are used.
pub enum KeyCode {}
pub struct KeyModifiers;
pub struct KeyEvent;
pub enum KeyEventKind {}
pub struct KeyEventState;
pub enum Event {}
pub struct MouseEvent;
pub enum MouseEventKind {}
pub enum MouseButton {}
pub enum MediaKeyCode {}
pub enum ModifierKeyCode {}
