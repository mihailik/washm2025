#[cfg(not(feature = "wasm-minimal"))]
mod command;
#[cfg(not(feature = "wasm-minimal"))]
#[cfg(feature = "ion")]
mod ion_bridge;
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
}

// Re-export _continue from wasm_async for the Host
pub use wasm_async::_continue;

