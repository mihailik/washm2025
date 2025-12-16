use crate::brush_core::error;

/// Host shim for interpreter error logging. Quiet by default; set WASHM_VERBOSE=1 to enable.
pub fn log_interp_error<T: std::fmt::Debug>(command: &T, err: &error::Error) {
    if std::env::var("WASHM_VERBOSE").is_ok() {
        eprintln!("WASHM: interp error: {:?}", command);
        eprintln!("WASHM: interp error details: {:?}", err);
    }
}
