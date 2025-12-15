use brush_parser::ast;
use crate::brush_core::error;

pub fn log_interp_error(command: &ast::Command, err: &error::Error) {
    println!("DEBUG interp: Error executing command: {:?}", command);
    println!("DEBUG interp: Error details: {:?}", err);
}
