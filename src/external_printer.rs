use std::fmt::Display;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

/// Default capacity for the external printer channel.
pub const EXTERNAL_PRINTER_DEFAULT_CAPACITY: usize = 20;

/// ExternalPrinter allows background tasks to send lines to be printed 
/// while the editor is active, without corrupting the current line buffer.
#[derive(Debug, Clone)]
pub struct ExternalPrinter<T>
where
    T: Display + Send + 'static,
{
    sender: UnboundedSender<T>,
}

impl<T> ExternalPrinter<T>
where
    T: Display + Send + 'static,
{
    /// Create a new ExternalPrinter.
    pub fn new() -> (Self, UnboundedReceiver<T>) {
        let (sender, receiver) = unbounded::<T>();
        (Self { sender }, receiver)
    }

    /// Send a line to be printed.
    pub fn print(&self, line: T) -> Result<(), String> {
        self.sender
            .unbounded_send(line)
            .map_err(|e| format!("Failed to send to external printer: {}", e))
    }
}
