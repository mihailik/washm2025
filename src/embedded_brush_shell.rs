use crate::brush_shell::args::CommandLineArgs;
use crate::brush_shell::entry;
use std::collections::HashMap;

pub fn run_custom(
    args: Vec<String>,
    extra_builtins: impl FnOnce(&mut HashMap<String, crate::brush_core::builtins::Registration>),
) {
    // Force initialization of startup parameters (read once from VFS)
    // Use the crate-local `host_startup` helpers directly rather than
    // depending on vendored stubs so we avoid fragile re-exports.
    let _ = crate::host_startup::startup_username();
    let _ = crate::host_startup::startup_uid();
    let _ = crate::host_startup::startup_gid();
    let _ = crate::host_startup::startup_hostname();

    let parsed_args = match CommandLineArgs::try_parse_from(args.iter().cloned()) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return;
        }
    };
    
    // Register extra builtins in global registry
    unsafe {
        let mut builtin_map = HashMap::new();
        extra_builtins(&mut builtin_map);
        entry::EXTRA_BUILTINS = Some(builtin_map);
    }

    // Run the async main and record its exit code to a file the host can inspect.
    crate::wasm_async::spawn(async move {
        let code = match entry::run_async(args, parsed_args).await {
            Ok(c) => c,
            Err(_) => 1u8,
        };
        // Best-effort write to a well-known file in the preopened FS.
        let _ = std::fs::write("/dev/host/washm/exit_code", format!("{}", code));
    });

    // Tick the runtime until the spawned task completes.
    crate::wasm_async::tick();
}
