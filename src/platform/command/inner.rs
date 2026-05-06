//! Reedline-owned terminal command primitives used when the
//! `crossterm-backend` feature is disabled. Phase C-2 will populate.

compile_error!(
    "src/platform/command/inner.rs has not been populated yet (Phase C-2). \
     Build with the `crossterm-backend` feature enabled (default) for now."
);

pub trait QueueableCommand {}
pub struct MoveTo(pub u16, pub u16);
pub struct MoveUp(pub u16);
pub struct Hide;
pub struct Show;
pub struct SavePosition;
pub struct RestorePosition;
pub struct Print<T>(pub T);
pub enum Attribute {
    Reset,
}
pub struct SetAttribute(pub Attribute);
pub struct SetForegroundColor(pub crate::platform::style::Color);
pub struct ResetColor;
pub struct Clear(pub ClearType);
pub enum ClearType {
    All,
    Purge,
    FromCursorDown,
}
