use std::io;
use std::sync::Arc;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/brush-core/src/lib.rs"]
pub mod brush_core;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/brush-builtins/src/lib.rs"]
pub mod brush_builtins;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/brush-parser/src/lib.rs"]
pub mod brush_parser;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/brush-interactive/src/lib.rs"]
pub mod brush_interactive;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/brush-shell/src/lib.rs"]
pub mod brush_shell;

#[cfg(target_os = "wasi")]
#[macro_use]
#[path = "../target/vendor/uucore/src/lib/lib.rs"]
pub mod uucore;

#[cfg(target_os = "wasi")]
// Re-export locale for uucore macros that use $crate::locale
pub use uucore::locale;

#[cfg(target_os = "wasi")]
pub mod bootstrap;
#[cfg(target_os = "wasi")]
pub mod bridges;
#[cfg(target_os = "wasi")]
pub mod interp_log;
#[cfg(target_os = "wasi")]
pub mod platform;
#[cfg(target_os = "wasi")]
pub mod wasm_interactive;

#[cfg(target_os = "wasi")]
pub mod shims;
#[cfg(target_os = "wasi")]
pub mod stubs_impl;

#[cfg(target_os = "wasi")]
mod async_test;
#[cfg(target_os = "wasi")]
pub mod brush_run;
#[cfg(target_os = "wasi")]
mod command;
#[cfg(target_os = "wasi")]
mod coreutils_builtins;
#[cfg(target_os = "wasi")]
mod coreutils_dispatch;

#[cfg(all(target_os = "wasi", feature = "wasm-test"))]
pub mod test_harness;

#[cfg(all(target_os = "wasi", feature = "wasm-test"))]
pub mod example_tests;

#[cfg(all(target_os = "wasi", feature = "wasm-test"))]
mod test_command;

// Guest REPL and command handling are implemented internally. Host-driven axy_submit removed.

// Host-driven `axy_submit` removed. The guest runs its own REPL via WASI `_start`.

/// Standard WASI Reactor initialization.
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn washm_init() {
    // 0. Initialize TTY bridge first (so it's the inner layer)
    let _ = bridges::tty::ensure_bridge();

    // 1.1 Load browser FS bridge (overrides protocol.js VFS in browser)
    let _ = bridges::fs::ensure_bridge();

    // 1. Load protocol (critical for VFS, wraps TTY)
    bootstrap::bootstrap_host_protocol();

    // 2. Load eval (critical for other bridges)
    let _ = bridges::eval::ensure_bridge();

    // 4. Close bootstrap FD to trigger host eval

    // 5. Initialize other bridges
    let _ = bridges::host::ensure_bridge();
    let _ = bridges::process::ensure_bridge();
    let _ = bridges::timer::ensure_bridge();

    // Always start the Brush REPL
    // The test command is registered as a builtin when wasm-test feature is enabled
    {
        // Start the Brush REPL immediately after initialization.
        // Errors are ignored; the REPL runs until stdin is closed.
        let _ = brush_run::run(&[]);
    }
}

// Re-export _continue from bridges::eval for the Host
#[cfg(target_os = "wasi")]
pub use crate::bridges::eval::_continue;

// Export platform setter for JavaScript bootstrap
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn washm_set_platform(is_browser: i32) {
    platform::set_platform(is_browser != 0);
}

#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn _start() {
    washm_init();
}
