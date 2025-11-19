use std::ffi::OsString;
use std::io;

pub fn get() -> io::Result<OsString> {
    Ok(OsString::from("wasi-host"))
}
