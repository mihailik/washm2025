#[cfg(feature = "crossterm-backend")]
pub(crate) mod bracketed_paste;
#[cfg(feature = "crossterm-backend")]
pub(crate) mod kitty;
pub mod semantic_prompt;

/// Return if the terminal supports the kitty keyboard enhancement protocol
///
/// Read more: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>
///
/// SIDE EFFECT: Touches the terminal file descriptors
#[cfg(feature = "crossterm-backend")]
pub fn kitty_protocol_available() -> bool {
    crossterm::terminal::supports_keyboard_enhancement().unwrap_or_default()
}

/// Return if the terminal supports the kitty keyboard enhancement protocol.
///
/// Without the `crossterm-backend` feature there is no terminal to query;
/// hosts driving reedline through a virtual terminal must report the
/// capability themselves.
#[cfg(not(feature = "crossterm-backend"))]
pub fn kitty_protocol_available() -> bool {
    false
}
