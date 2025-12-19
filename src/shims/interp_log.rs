use crate::brush_core::error;

/// Host shim that augments interpreter logging. The vendored code calls this
/// with `(command, &err, shell)`; forward to the minimal WASM-side logger.
pub fn log_interp_error<T: std::fmt::Debug>(command: &T, err: &error::Error, _shell: &crate::brush_interactive::ShellRef) {
    // Keep WASM-side behavior: concise output. Host builds may extend this.
    crate::interp_log::log_interp_error(command, err);
}
