mod example;
mod simple_match;

use crate::StyledText;

pub use example::ExampleHighlighter;
pub use simple_match::SimpleMatchHighlighter;
/// The syntax highlighting trait. Implementers of this trait will take in the current string and then
/// return a `StyledText` object, which represents the contents of the original line as styled strings
pub trait Highlighter {
    /// The action that will handle the current buffer as a line and return the corresponding `StyledText` for the buffer
    ///
    /// Cursor position as byte offsets in the string
    fn highlight<'a>(
        &'a self,
        line: &'a str,
        cursor: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = StyledText> + 'a>>;
}
