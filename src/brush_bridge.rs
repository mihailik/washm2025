use crate::wasm_async::WasmHostFuture;
use std::future::Future;

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
        // No-op for now; brush builtins will register a `host-eval` command
        // that delegates to `host_eval` at runtime. This bridge function
        // remains for compatibility with legacy initialization flows.
    }

    Ok(0)
}
