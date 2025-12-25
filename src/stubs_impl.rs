use crate::brush_builtins::BuiltinSet;
use crate::brush_builtins::ShellBuilderExt;
use crate::brush_core::{ShellBuilder, ShellBuilderState};
use crate::brush_shell::brushctl::ShellBuilderBrushBuiltinExt;
use std::collections::HashMap;

/// Portable helper implementations for platform-limited functionality.
/// These are invoked by small trampolines injected into vendored stubs
/// so the real logic lives under `src/` where it is reviewed and tested.

// --- Startup Helpers ---
pub fn startup_username() -> String {
    crate::bridges::host::startup_username()
}

pub fn startup_uid() -> u32 {
    crate::bridges::host::startup_uid()
}

pub fn startup_gid() -> u32 {
    crate::bridges::host::startup_gid()
}

pub fn startup_hostname() -> String {
    crate::bridges::host::startup_hostname()
}

// --- UID/GID Helpers ---
// These functions read from the startup metadata which includes os.userInfo()
// The host module already caches the startup JSON, so we just delegate to it

pub fn get_current_uid() -> Result<u32, crate::brush_core::error::Error> {
    Ok(crate::bridges::host::startup_uid())
}

pub fn get_current_gid() -> Result<u32, crate::brush_core::error::Error> {
    Ok(crate::bridges::host::startup_gid())
}

pub fn get_effective_uid() -> Result<u32, crate::brush_core::error::Error> {
    Ok(crate::bridges::host::startup_uid())
}

pub fn get_effective_gid() -> Result<u32, crate::brush_core::error::Error> {
    Ok(crate::bridges::host::startup_gid())
}

// --- OpenFile Helpers ---
pub fn clone_open_file(
    f: &crate::brush_core::openfiles::OpenFile,
) -> crate::brush_core::openfiles::OpenFile {
    match f.try_clone() {
        Ok(v) => v,
        Err(_) => {
            // Best-effort fallback on platforms where dup/clone is unsupported
            if matches!(f, crate::brush_core::openfiles::OpenFile::Stdin(_)) {
                crate::brush_core::openfiles::OpenFile::Stdin(std::io::stdin())
            } else if matches!(f, crate::brush_core::openfiles::OpenFile::Stdout(_)) {
                crate::brush_core::openfiles::OpenFile::Stdout(std::io::stdout())
            } else if matches!(f, crate::brush_core::openfiles::OpenFile::Stderr(_)) {
                crate::brush_core::openfiles::OpenFile::Stderr(std::io::stderr())
            } else {
                // Try to open /dev/null if we can't clone
                open_null_file()
                    .map(crate::brush_core::openfiles::OpenFile::File)
                    .unwrap_or_else(|_| {
                        crate::brush_core::openfiles::OpenFile::Stdin(std::io::stdin())
                    })
            }
        }
    }
}

// --- Extra Builtins Helpers ---
pub static mut EXTRA_BUILTINS: Option<HashMap<String, crate::brush_core::builtins::Registration>> =
    None;

pub fn configure_shell<S: ShellBuilderState>(
    builder: ShellBuilder<S>,
    builtins: BuiltinSet,
) -> ShellBuilder<S> {
    let mut builder = builder.default_builtins(builtins).brush_builtins();

    if let Some(Some(extras)) = unsafe { std::ptr::addr_of!(EXTRA_BUILTINS).as_ref() } {
        for (name, reg) in extras {
            builder = builder.builtin(name.clone(), reg.clone());
        }
    }
    builder
}

pub fn open_null_file() -> Result<std::fs::File, crate::brush_core::error::Error> {
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
        crate::brush_core::error::Error::from(e)
    })
}

pub fn continue_process(
    _pid: crate::brush_core::sys::process::ProcessId,
) -> Result<(), crate::brush_core::error::Error> {
    Ok(())
}

pub fn kill_process(
    _pid: crate::brush_core::sys::process::ProcessId,
    _signal: crate::brush_core::traps::TrapSignal,
) -> Result<(), crate::brush_core::error::Error> {
    Ok(())
}

pub fn inject_fds(
    files: Vec<(
        crate::brush_core::ShellFd,
        crate::brush_core::openfiles::OpenFile,
    )>,
) -> Result<(), crate::brush_core::error::Error> {
    drop(files);
    Ok(())
}

// --- Process Spawning VFS Bridge ---

use std::io::{Read, Write};

/// Spawn a process via the VFS bridge
pub fn spawn_process(command: std::process::Command) -> std::io::Result<GuestChild> {
    // Process bridge is installed automatically via protocol.js bootstrap

    // Build spawn request JSON
    let mut request = serde_json::Map::new();

    // Get program name properly
    let program = command.get_program().to_string_lossy().to_string();
    request.insert("program".to_string(), serde_json::Value::String(program));

    // Get args
    let args: Vec<String> = command
        .get_args()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    request.insert(
        "args".to_string(),
        serde_json::Value::Array(
            args.iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );

    // Get env - if custom env is set, use it, otherwise inherit from parent
    // Note: We pass empty object to signal "use inherited env" to the host
    let mut env_map = serde_json::Map::new();
    for (key, value) in command.get_envs() {
        if let Some(v) = value {
            env_map.insert(
                key.to_string_lossy().to_string(),
                serde_json::Value::String(v.to_string_lossy().to_string()),
            );
        }
    }
    // Only include env if we have explicit overrides
    if !env_map.is_empty() {
        request.insert("env".to_string(), serde_json::Value::Object(env_map));
    }

    // Get cwd
    if let Some(cwd) = command.get_current_dir() {
        request.insert(
            "cwd".to_string(),
            serde_json::Value::String(cwd.to_string_lossy().to_string()),
        );
    }

    // Set up fds - use inherit by default so output goes to terminal
    // Only use pipe if brush has explicitly set up piping
    let mut fds_json = serde_json::Map::new();
    fds_json.insert(
        "0".to_string(),
        serde_json::Value::String("inherit".to_string()),
    );
    fds_json.insert(
        "1".to_string(),
        serde_json::Value::String("inherit".to_string()),
    );
    fds_json.insert(
        "2".to_string(),
        serde_json::Value::String("inherit".to_string()),
    );

    request.insert("fds".to_string(), serde_json::Value::Object(fds_json));

    let request_json = serde_json::Value::Object(request);
    let request_str = serde_json::to_string(&request_json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    // Write to /dev/host/process/spawn.json
    let mut spawn_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/dev/host/process/spawn.json")?;

    spawn_file.write_all(request_str.as_bytes())?;
    spawn_file.flush()?;
    drop(spawn_file);

    // Read response (PID)
    let mut response_file = std::fs::File::open("/dev/host/process/spawn.json")?;
    let mut pid_str = String::new();
    response_file.read_to_string(&mut pid_str)?;

    let pid = pid_str
        .trim()
        .parse::<u32>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(GuestChild { pid })
}

/// Guest-side Child process handle
pub struct GuestChild {
    pid: u32,
}

impl GuestChild {
    pub fn id(&self) -> Option<u32> {
        Some(self.pid)
    }

    pub async fn wait(&mut self) -> std::io::Result<crate::shims::exit_status::ExitStatus> {
        use std::time::Duration;

        let status_path = format!("/dev/host/process/{}/status", self.pid);

        // Poll for status with backoff
        let mut poll_interval = Duration::from_millis(10);
        let max_interval = Duration::from_millis(100);

        loop {
            match std::fs::File::open(&status_path) {
                Ok(mut file) => {
                    let mut status_str = String::new();
                    match file.read_to_string(&mut status_str) {
                        Ok(_) => {
                            if !status_str.is_empty() {
                                // Parse status JSON
                                if let Ok(status) =
                                    serde_json::from_str::<serde_json::Value>(&status_str)
                                {
                                    if status["exited"].as_bool() == Some(true) {
                                        let code = status["code"].as_i64().unwrap_or(-1) as i32;
                                        return self.create_exit_status(code);
                                    }
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
                Err(_) => {}
            }

            // Sleep and apply backoff using async sleep (non-blocking)
            crate::bridges::timer::async_sleep(poll_interval).await.ok();
            poll_interval = (poll_interval * 2).min(max_interval);
        }
    }

    pub async fn wait_with_output(self) -> std::io::Result<crate::shims::exit_status::Output> {
        use std::time::Duration;

        let stdout_path = format!("/dev/host/process/{}/stdout", self.pid);
        let stderr_path = format!("/dev/host/process/{}/stderr", self.pid);
        let status_path = format!("/dev/host/process/{}/status", self.pid);

        // Poll for status with backoff (same logic as wait() but inline)
        let mut poll_interval = Duration::from_millis(10);
        let max_interval = Duration::from_millis(100);
        let mut exit_code;

        loop {
            match std::fs::File::open(&status_path) {
                Ok(mut file) => {
                    let mut status_str = String::new();
                    match file.read_to_string(&mut status_str) {
                        Ok(_) => {
                            if !status_str.is_empty() {
                                // Parse status JSON
                                if let Ok(status) =
                                    serde_json::from_str::<serde_json::Value>(&status_str)
                                {
                                    if status["exited"].as_bool() == Some(true) {
                                        exit_code = status["code"].as_i64().unwrap_or(-1) as i32;
                                        break;
                                    }
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
                Err(_) => {}
            }

            // Sleep and apply backoff using async sleep (non-blocking)
            crate::bridges::timer::async_sleep(poll_interval).await.ok();
            poll_interval = (poll_interval * 2).min(max_interval);
        }

        // Read stdout
        let stdout = std::fs::read(&stdout_path).unwrap_or_default();

        // Read stderr
        let stderr = std::fs::read(&stderr_path).unwrap_or_default();

        // Create ExitStatus directly without recursive call
        let status = self.create_exit_status(exit_code)?;

        Ok(crate::shims::exit_status::Output {
            status,
            stdout,
            stderr,
        })
    }

    // Create ExitStatus
    fn create_exit_status(&self, code: i32) -> std::io::Result<crate::shims::exit_status::ExitStatus> {
        Ok(crate::shims::exit_status::ExitStatus::from_raw(code))
    }
}
