use crate::brush_shell::args::CommandLineArgs;
use crate::brush_shell::entry;
use std::collections::HashMap;

/// WASHM version, set at build time
const WASHM_VERSION: &str = env!("CARGO_PKG_VERSION");

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

    // Special handling for --version: print washm version instead of brush version
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("washm {}", WASHM_VERSION);
        // Exit cleanly by writing exit code 0
        let _ = std::fs::write("/dev/host/washm/exit_code", "0");
        return;
    }

    let parsed_args = match CommandLineArgs::try_parse_from(args.iter().cloned()) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            // Exit cleanly with error code 1 to prevent hanging
            let _ = std::fs::write("/dev/host/washm/exit_code", "1");
            return;
        }
    };
    
    // Register extra builtins in global registry
    unsafe {
        let mut builtin_map = HashMap::new();
        extra_builtins(&mut builtin_map);
        crate::stubs_impl::EXTRA_BUILTINS = Some(builtin_map);
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
