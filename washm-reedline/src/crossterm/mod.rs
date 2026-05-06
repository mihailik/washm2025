//! Glue between reedline's [`crate::platform`] vocabulary and the
//! external [`::crossterm`] crate.
//!
//! This module is only compiled when the `crossterm-backend` feature is
//! enabled. It will gradually accumulate `From` conversions and
//! adapter helpers as Phase B / Phase C of the WASM publication plan
//! progress; for now it is intentionally empty so the module path
//! exists and `lib.rs` can declare it.
