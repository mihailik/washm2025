//! Terminal text styling primitives (color and attributes).
//!
//! Phase A: native re-exports of [`crossterm::style`] types when the
//! `crossterm-backend` feature is enabled. Phase C populates `inner`
//! with reedline-owned definitions for non-`crossterm` targets.

#[cfg(feature = "crossterm-backend")]
pub use crossterm::style::{Attribute, Attributes, Color};

#[cfg(not(feature = "crossterm-backend"))]
mod inner;

#[cfg(not(feature = "crossterm-backend"))]
pub use inner::{Attribute, Attributes, Color};
