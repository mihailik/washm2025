//! Reedline-owned style type definitions used when the
//! `crossterm-backend` feature is disabled. Phase C will populate.

compile_error!(
    "src/platform/style/inner.rs has not been populated yet (Phase C). \
     Build with the `crossterm-backend` feature enabled (default) for now."
);

pub enum Color {}
pub enum Attribute {}
pub struct Attributes;
