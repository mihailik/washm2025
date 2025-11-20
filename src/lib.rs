#[cfg(not(feature = "wasm-minimal"))]
mod command;
#[cfg(not(feature = "wasm-minimal"))]
pub mod brush_bridge;
#[cfg(not(feature = "wasm-minimal"))]
mod uutils_bridge;
mod wasm_async;
#[cfg(not(feature = "wasm-minimal"))]
mod async_test;
#[cfg(not(feature = "wasm-minimal"))]
mod jobs;

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
}


// Re-export _continue from wasm_async for the Host
pub use wasm_async::_continue;

