use nu_ansi_term::Style;
use std::ops::Range;

/// A span of source code, with positions in bytes
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct Span {
    /// The starting position of the span, in bytes
    pub start: usize,

    /// The ending position of the span, in bytes
    pub end: usize,
}

impl Span {
    /// Creates a new `Span` from start and end inputs.
    /// The end parameter must be greater than or equal to the start parameter.
    ///
    /// # Panics
    /// If `end < start`
    pub fn new(start: usize, end: usize) -> Span {
        assert!(
            end >= start,
            "Can't create a Span whose end < start, start={start}, end={end}"
        );

        Span { start, end }
    }
}

use std::future::Future;
use std::pin::Pin;

/// A trait that defines how to convert some text and a position to a list of potential completions in that position.
/// The text could be a part of the whole line, and the position is the index of the end of the text in the original line.
pub trait Completer {
    /// the action that will take the line and position and convert it to a vector of completions, which include the
    /// span to replace and the contents of that replacement
    fn complete<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<Suggestion>> + 'a>>;

    /// same as [`Completer::complete`] but it will return a vector of ranges of the strings
    /// the suggestions are based on
    fn complete_with_base_ranges<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
    ) -> Pin<Box<dyn Future<Output = (Vec<Suggestion>, Vec<Range<usize>>)> + 'a>> {
        Box::pin(async move {
            let mut ranges = vec![];
            let suggestions = self.complete(line, pos).await;
            for suggestion in &suggestions {
                ranges.push(suggestion.span.start..suggestion.span.end);
            }
            ranges.dedup();
            (suggestions, ranges)
        })
    }

    /// action that will return a partial section of available completions
    /// this command comes handy when trying to avoid to pull all the data at once
    /// from the completer
    fn partial_complete<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
        start: usize,
        offset: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<Suggestion>> + 'a>> {
        Box::pin(async move {
            self.complete(line, pos)
                .await
                .into_iter()
                .skip(start)
                .take(offset)
                .collect()
        })
    }

    /// number of available completions
    fn total_completions<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
    ) -> Pin<Box<dyn Future<Output = usize> + 'a>> {
        Box::pin(async move { self.complete(line, pos).await.len() })
    }
}

/// Suggestion returned by the Completer
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// String replacement that will be introduced to the the buffer
    pub value: String,
    /// If given, overrides `value` as text displayed to user
    pub display_override: Option<String>,
    /// Optional description for the replacement
    pub description: Option<String>,
    /// Optional style for the replacement
    pub style: Option<Style>,
    /// Optional vector of strings in the suggestion. These can be used to
    /// represent examples coming from a suggestion
    pub extra: Option<Vec<String>>,
    /// Replacement span in the buffer
    pub span: Span,
    /// Whether to append a space after selecting this suggestion.
    /// This helps to avoid that a completer repeats the complete suggestion.
    pub append_whitespace: bool,
    /// Indices of the graphemes in the suggestion that matched the typed text.
    /// Useful if using fuzzy matching.
    pub match_indices: Option<Vec<usize>>,
}

impl Suggestion {
    /// Get value to display to user for this suggestion
    pub fn display_value(&self) -> &str {
        self.display_override.as_ref().unwrap_or(&self.value)
    }
}
