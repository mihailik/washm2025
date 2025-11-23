use std::io;
use crossterm::platform::{TtyDelegates, set_tty_delegates};
mod command;
pub mod brush_bridge;
mod uutils_bridge;
mod async_test;
mod jobs;

pub mod wasm_async;
pub mod tty;

// Guest REPL and command handling are implemented internally. Host-driven axy_submit removed.

// Host-driven `axy_submit` removed. The guest runs its own REPL via WASI `_start`.

/// Standard WASI Reactor initialization.
#[no_mangle]
pub extern "C" fn _initialize() {
    // Initialization logic here
    {
        // Start the Brush REPL immediately after initialization.
        // Errors are ignored; the REPL runs until stdin is closed.
        let _ = brush_bridge::run(&[]);
    }

    // Register the platform delegates with the crossterm shim so it can
    // call into `washm::tty` without creating a dependency cycle.
    set_tty_delegates(TtyDelegates {
        enable_raw_mode: Box::new(|| {
            crate::tty::enable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }),
        disable_raw_mode: Box::new(|| {
            crate::tty::disable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }),
        size: Box::new(|| {
            crate::tty::size().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }),
    });
}


// Re-export _continue from wasm_async for the Host
pub use crate::wasm_async::_continue;

