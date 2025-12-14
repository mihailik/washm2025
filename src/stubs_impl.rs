use crate::brush_core::{error, openfiles, ShellFd};
use std::collections::HashMap;

/// Portable helper implementations for platform-limited functionality.
/// These are invoked by small trampolines injected into vendored stubs
/// so the real logic lives under `src/` where it is reviewed and tested.

// --- Startup Helpers ---
pub fn startup_username() -> String {
    crate::host_startup::startup_username()
}

pub fn startup_uid() -> u32 {
    crate::host_startup::startup_uid()
}

pub fn startup_gid() -> u32 {
    crate::host_startup::startup_gid()
}

pub fn startup_hostname() -> String {
    crate::host_startup::startup_hostname()
}

// --- OpenFile Helpers ---
pub fn clone_open_file(f: &openfiles::OpenFile) -> openfiles::OpenFile {
    match f.try_clone() {
        Ok(v) => v,
        Err(_) => {
            // Best-effort fallback on platforms where dup/clone is unsupported
            if matches!(f, openfiles::OpenFile::Stdin(_)) {
                openfiles::OpenFile::Stdin(std::io::stdin())
            } else if matches!(f, openfiles::OpenFile::Stdout(_)) {
                openfiles::OpenFile::Stdout(std::io::stdout())
            } else if matches!(f, openfiles::OpenFile::Stderr(_)) {
                openfiles::OpenFile::Stderr(std::io::stderr())
            } else {
                // Try to open /dev/null if we can't clone
                open_null_file().map(openfiles::OpenFile::File).unwrap_or_else(|_| openfiles::OpenFile::Stdin(std::io::stdin()))
            }
        }
    }
}

// --- Extra Builtins Helpers ---
pub static mut EXTRA_BUILTINS: Option<HashMap<String, crate::brush_core::builtins::Registration>> = None;

// We need a version that takes ownership and returns ownership, matching the builder pattern
pub fn add_extra_builtins<S: crate::brush_core::ShellBuilderState>(mut builder: crate::brush_core::ShellBuilder<S>) -> crate::brush_core::ShellBuilder<S> {
    if let Some(Some(extras)) = unsafe { std::ptr::addr_of!(EXTRA_BUILTINS).as_ref() } {
        for (name, reg) in extras {
            builder = builder.builtin(name.clone(), reg.clone());
        }
    }
    builder
}

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
