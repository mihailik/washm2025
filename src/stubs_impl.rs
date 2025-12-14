use crate::brush_core::{error, openfiles, ShellFd};

/// Portable helper implementations for platform-limited functionality.
/// These are invoked by small trampolines injected into vendored stubs
/// so the real logic lives under `src/` where it is reviewed and tested.
pub fn open_null_file() -> Result<std::fs::File, error::Error> {
    println!("DEBUG: stubs_impl::open_null_file called");
    let res = std::fs::File::options()
        .read(true)
        .write(true)
        .open("/dev/null");

    if let Err(e) = &res {
        println!("DEBUG: open(/dev/null) failed: {:?}", e);
        panic!("CRASH: open(/dev/null) failed with error: {:?}", e);
    } else {
        println!("DEBUG: open(/dev/null) succeeded");
    }

    res.map_err(|e| {
            println!("DEBUG: open_null_file final error: {:?}", e);
            error::Error::from(e)
        })
}

pub fn continue_process(_pid: crate::brush_core::sys::process::ProcessId) -> Result<(), error::Error> {
    Ok(())
}

pub fn kill_process(
    _pid: crate::brush_core::sys::process::ProcessId,
    _signal: crate::brush_core::traps::TrapSignal,
) -> Result<(), error::Error> {
    Ok(())
}

pub fn inject_fds(
    files: Vec<(ShellFd, openfiles::OpenFile)>,
) -> Result<(), error::Error> {
    drop(files);
    Ok(())
}
