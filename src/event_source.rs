use crate::platform::event::Event;
use std::io;

/// Produces raw crossterm events, asynchronously.
pub trait EventSource: Send {
    /// Wait for the next event.
    fn next_event(&mut self) -> BoxFuture<'_, io::Result<Event>>;
}

/// A pinned-boxed future for trait returns.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// The default implementation using crossterm's blocking API wrapped in a future if needed,
/// or a dedicated async stream if available.
///
/// Only available when the `crossterm-backend` feature is enabled.
#[cfg(feature = "crossterm-backend")]
pub struct CrosstermEventSource;

#[cfg(feature = "crossterm-backend")]
impl EventSource for CrosstermEventSource {
    fn next_event(&mut self) -> BoxFuture<'_, io::Result<Event>> {
        Box::pin(async {
            // For now, we still use the blocking read in a fake async way 
            // since we are in a single-threaded cooperative model anyway.
            // Platform-specific non-blocking versions would go here.
            crossterm::event::read()
        })
    }
}
