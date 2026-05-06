//! Platform-neutral type vocabulary for reedline-async.
//!
//! This module is the canonical home for the data types reedline operates
//! on (key codes, events, colors, ANSI commands, …). Everything in this
//! module is `Send`, target-agnostic, and **does not depend on the
//! operating system**.
//!
//! ## Why this exists
//!
//! The historical [`crossterm`] crate is monolithic: it couples the
//! data-type vocabulary (`KeyCode`, `Event`, `Color`, …) with deeply
//! OS-specific I/O (POSIX `termios`, Windows console APIs, polling).
//! That coupling means `crossterm` does not compile for
//! `wasm32-unknown-unknown` or `wasm32-wasip1`, even when only the data
//! types are needed.
//!
//! Reedline-async therefore owns its own type vocabulary in
//! [`crate::platform`]. Native targets re-export the corresponding
//! `crossterm` types so the public API and downstream behaviour is
//! identical to vanilla reedline. WebAssembly targets fall back to a
//! reedline-internal definition (filled in incrementally; see Phase C of
//! the WASM publication plan).
//!
//! See also [`crate::crossterm`] for the conversion / glue layer that
//! exists only when the `crossterm-backend` feature is enabled.

pub mod command;
pub mod cursor;
pub mod event;
pub mod style;
