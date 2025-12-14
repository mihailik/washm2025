use crate::wasm_async::WasmHostFuture;
use crate::brush_core::builtins::{SimpleCommand, ContentType, ContentOptions};
use crate::brush_core::{commands, results, error};

struct CatCommand;

impl SimpleCommand for CatCommand {
    fn get_content(name: &str, _content_type: ContentType, _options: &ContentOptions) -> Result<String, error::Error> {
        Ok(format!("{} - concatenate files and print on the standard output", name))
    }

    fn execute<I: Iterator<Item = S>, S: AsRef<str>>(
        _context: commands::ExecutionContext<'_>,
        args: I,
    ) -> Result<results::ExecutionResult, error::Error> {
        let args_vec: Vec<String> = args.map(|s| s.as_ref().to_string()).collect();
        
        let code = crate::uutils_bridge::UutilsBridge::run("cat", &args_vec);
        Ok(results::ExecutionResult::new(code as u8))
    }
}

/// Minimal Bridge for Brush
/// This file provides a `host_eval` helper and `run` entrypoint for the
/// `brush` feature. Real integration should replace the stub with actual
/// brush runtime initialization and async wiring.
pub async fn host_eval(js: &str) -> Result<String, &'static str> {
    WasmHostFuture::new(js).await
}

pub fn run(_args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    // Try to get terminal size to verify TTY integration
    if let Ok((_cols, _rows)) = crate::tty::size() {
        // TTY is available
    }

    // Get arguments to pass to brush
    let raw_args: Vec<String> = std::env::args().collect();
    
    // Skip host args (executable name, possibly washm.js and wasm file)
    let mut start_index = 1;
    if raw_args.len() > start_index && raw_args[start_index].ends_with(".js") {
        start_index += 1;
    }
    if raw_args.len() > start_index && raw_args[start_index].ends_with(".wasm") {
        start_index += 1;
    }

    // Collect user arguments
    let user_args: Vec<String> = raw_args.iter().skip(start_index).cloned().collect();
    
    // Change to the host's starting directory
    if let Ok(start_pwd) = std::fs::read_to_string("/dev/host/washm/start_pwd") {
        let start_pwd = start_pwd.trim();
        if !start_pwd.is_empty() {
            let _ = std::env::set_current_dir(start_pwd);
        }
    }

    // Build arguments for brush
    let mut args = Vec::new();
    args.push("brush".to_string());

    // We need to force the Minimal backend because the default Reedline backend
    // requires a TTY, which is not available in the WASI environment (piped stdin).
    args.push("--input-backend".to_string());
    args.push("minimal".to_string());
    
    // If we have user args, run them as a single command with -c
    if !user_args.is_empty() {
        args.push("-c".to_string());
        args.push(user_args.join(" "));
    }

    let extra_builtins = |map: &mut std::collections::HashMap<String, crate::brush_core::builtins::Registration>| {
        map.insert("cat".to_string(), crate::brush_core::builtins::simple_builtin::<CatCommand>());

        // Coreutils wrappers
        map.insert("ls".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::LsCommand>());
        map.insert("cp".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::CpCommand>());
        map.insert("mv".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::MvCommand>());
        map.insert("rm".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::RmCommand>());
        map.insert("mkdir".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::MkdirCommand>());
        map.insert("rmdir".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::RmdirCommand>());
        map.insert("touch".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::TouchCommand>());
        // map.insert("pwd".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::PwdCommand>());
        
        #[cfg(feature = "wasm-test")]
        map.insert("test".to_string(), crate::brush_core::builtins::simple_builtin::<crate::test_command::TestCommand>());
    };

    crate::brush_shell::embedded::run_custom(args, extra_builtins);
    
    Ok(0)
}
