// Stub ctrlc crate for WASI compatibility
// The real ctrlc uses platform-specific signal handling which doesn't work in WASI

pub fn set_handler<F>(_handler: F) -> Result<(), Error>
where
    F: FnMut() + 'static + Send,
{
    // No-op in WASI
    Ok(())
}

#[derive(Debug)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ctrlc not supported in WASI")
    }
}

impl std::error::Error for Error {}
