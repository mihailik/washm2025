use crate::wasm_async::WasmHostFuture;
use std::future::Future;
use brush_shell::brush_core::builtins::{self, SimpleCommand, ContentType};
use brush_shell::brush_core::{commands, results, error};

struct CatCommand;

impl SimpleCommand for CatCommand {
    fn get_content(name: &str, _content_type: ContentType) -> Result<String, error::Error> {
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

pub fn run(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    // When brush-fork is added, register `host_eval` and start the interactive
    // shell. For now, this is a no-op stub that returns success.
    {
        // Start brush's `run()` entrypoint. `brush-shell` exposes `entry::run`.
        // Note: this will process arguments and may call `process::exit` in
        // some configurations; we catch and map it into a return value.
        // We intentionally keep the call simple for now.
        #[allow(unused_must_use)]
        {
            // run() will exit the process in native builds; in wasm it should
            // return normally if configured for embedded use.
            
            // We need to force the Minimal backend because the default Reedline backend
            // requires a TTY, which is not available in the WASI environment (piped stdin).
            let raw_args: Vec<String> = std::env::args().collect();
            let mut args = Vec::new();
            args.push("brush".to_string());

            // Inject --input-backend minimal at the beginning of arguments
            // to ensure it's parsed as a shell option, not a script argument.
            args.push("--input-backend".to_string());
            args.push("minimal".to_string());

            // Heuristic to skip host args (node, axy.js, wasm_file)
            // axy.js passes process.argv which includes: [node, axy.js, wasm_file, ...user_args]
            let mut start_index = 1; // Always skip argv[0] (runtime/executable name)

            if raw_args.len() > start_index && raw_args[start_index].ends_with(".js") {
                start_index += 1;
            }

            if raw_args.len() > start_index && raw_args[start_index].ends_with(".wasm") {
                start_index += 1;
            }

            for arg in raw_args.iter().skip(start_index) {
                args.push(arg.clone());
            }

            let mut extra_builtins = Vec::new();
            extra_builtins.push((
                "cat".to_string(),
                brush_shell::brush_core::builtins::simple_builtin::<CatCommand>(),
            ));

            brush_shell::entry::run_custom(args, extra_builtins);
        }
    }

    Ok(0)
}
