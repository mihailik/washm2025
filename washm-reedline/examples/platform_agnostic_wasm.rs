//! A demo showing how to use Reedline in a platform-agnostic way (e.g. WASM)
//! by providing custom implementations for `TerminalControl` and `EventSource`.
//!
//! This example doesn't actually run in a browser, but it implements the traits
//! as if it were communicating with a Javascript host.

use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use reedline::{
    BoxFuture, DefaultPrompt, EventSource, Reedline, Signal, TerminalControl,
};
use std::io::{Result, Write};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

/// A mock TerminalControl for a WASM/Web environment.
/// In a real WASM app, these methods would call into JS via wasm-bindgen.
struct WebTerminalControl {
    width: u16,
    height: u16,
}

impl TerminalControl for WebTerminalControl {
    fn enter_raw_mode(&mut self) -> Result<()> {
        println!("[JS] Entering raw mode (e.g. enabling xterm.js focus)");
        Ok(())
    }

    fn exit_raw_mode(&mut self) -> Result<()> {
        println!("[JS] Exiting raw mode");
        Ok(())
    }

    fn terminal_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn cursor_position(&self) -> (u16, u16) {
        // In WASM, the host usually tracks this
        (0, 0)
    }
}

/// A mock EventSource that receives events from a channel.
/// In a real WASM app, a JS 'keydown' listener would push into this channel.
struct WebEventSource {
    receiver: UnboundedReceiver<crossterm::event::Event>,
}

impl EventSource for WebEventSource {
    fn next_event(&mut self) -> BoxFuture<'_, Result<crossterm::event::Event>> {
        Box::pin(async {
            self.receiver
                .try_next()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Channel closed"))?
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF"))
        })
    }
}

/// a simple Writer that "flushes" to a hypothetical JS terminal
struct WebWriter;
impl Write for WebWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        // In WASM, this would call terminal.write(utf8_decode(buf))
        std::io::stdout().write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        std::io::stdout().flush()
    }
}

fn main() {
    let (tx, rx) = unbounded();
    
    // Simulate a keypress from JS
    tx.unbounded_send(crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    ))).unwrap();
    tx.unbounded_send(crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ))).unwrap();

    let terminal_control = Box::new(WebTerminalControl { width: 80, height: 24 });
    let event_source = Box::new(WebEventSource { receiver: rx });
    let painter_writer = Box::new(WebWriter);

    // We instantiate Reedline by injecting our custom platform traits.
    // The library itself remains clean of WASM-specific dependencies.
    let mut line_editor = Reedline::<Box<dyn Write + Send>>::create()
        .with_terminal_control(terminal_control)
        .with_event_source(event_source)
        .with_painter_writer(painter_writer);

    let prompt = DefaultPrompt::default();

    println!("Running platform-agnostic demo...");
    
    // Use an executor to run the async loop
    block_on(async {
        match line_editor.read_line(&prompt).await {
            Ok(Signal::Success(buffer)) => {
                println!("\nReceived buffer: {}", buffer);
            }
            _ => println!("\nInterrupted"),
        }
    });
}

