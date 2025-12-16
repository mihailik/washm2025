use crate::brush_core::error;

pub fn log_interp_error<T: std::fmt::Debug>(command: &T, err: &error::Error) {
    println!("DEBUG interp: Error executing command: {:?}", command);
    println!("DEBUG interp: Error details: {:?}", err);
}
