use crate::brush_core::error;

pub fn log_interp_error<T: std::fmt::Debug>(command: &T, err: &error::Error) {
    // Keep wasm-side output concise. The host shim will print context-rich snippets when
    // available; only print the error and, if verbose is enabled, the raw AST.
    println!("error: {}", err);
    if std::env::var("WASHM_VERBOSE").is_ok() {
        println!("Raw command AST: {:?}", command);
    }
}
