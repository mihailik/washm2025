use super::super::wasm_async::{eval_blocking, WasmHostFuture};
use std::io::{self, Write};
use serde::Deserialize;
use super::{InputBackend, ReadResult, InteractivePrompt, ShellError, ShellRef};

const BRIDGE_JS: &str = include_str!("tty/bridge.js");

#[derive(Clone)]
pub struct WasmInputBackend {
    initialized: bool,
}

impl InputBackend for WasmInputBackend {
    async fn read_line(&mut self, _shell: &ShellRef, prompt: InteractivePrompt) -> Result<ReadResult, ShellError> {
        match self.read_line_internal(&prompt.prompt).await {
            Ok(s) => Ok(ReadResult::Input(s)),
            Err(e) if e == "EOF" => Ok(ReadResult::Eof),
            Err(e) if e == "Interrupted" => Ok(ReadResult::Interrupted),
            Err(e) => Err(ShellError::from(std::io::Error::new(std::io::ErrorKind::Other, e))),
        }
    }
}

impl WasmInputBackend {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    async fn ensure_initialized(&mut self) -> Result<(), String> {
        if !self.initialized {
            let js = format!("{} //# sourceURL=washm-tty-bridge.js", BRIDGE_JS);
            let fut = WasmHostFuture::new(&js);
            let res = fut.await.map_err(|e| e.to_string())?;
            
            #[derive(Deserialize)]
            struct InitResult {
                ok: bool,
                error: Option<String>,
            }
            
            let init_res: InitResult = serde_json::from_str(&res).map_err(|e| format!("JSON error: {}", e))?;
            if !init_res.ok {
                return Err(init_res.error.unwrap_or_else(|| "Unknown init error".to_string()));
            }

            self.initialized = true;
        }
        Ok(())
    }

    pub async fn read_line_internal(&mut self, prompt: &str) -> Result<String, String> {
        if let Err(e) = self.ensure_initialized().await {
            return Err(e);
        }
        
        print!("{}", prompt);
        io::stdout().flush().map_err(|e| e.to_string())?;

        let mut line = String::new();
        // Simple cursor tracking (only at end of line supported for now)
        
        loop {
            let fut = WasmHostFuture::new("washm_tty.read()");
            let res_json = fut.await.map_err(|e| e.to_string())?;
            
            #[derive(Deserialize)]
            struct HostResult {
                ok: bool,
                result: Option<String>, // read returns string or null (which is Option<String>)
                error: Option<String>,
            }

            let host_res: HostResult = serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;
            
            if !host_res.ok {
                return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
            }
            
            let chunk = host_res.result;
            
            match chunk {
                None => {
                    if line.is_empty() {
                        return Err("EOF".to_string());
                    }
                    return Ok(line);
                },
                Some(s) => {
                    for c in s.chars() {
                        match c {
                            '\r' | '\n' => {
                                println!();
                                return Ok(line);
                            }
                            '\x08' | '\x7f' => { // Backspace
                                if !line.is_empty() {
                                    line.pop();
                                    print!("\x08 \x08"); 
                                    io::stdout().flush().map_err(|e| e.to_string())?;
                                }
                            }
                            '\x03' => { // Ctrl+C
                                return Err("Interrupted".to_string());
                            }
                            _ => {
                                line.push(c);
                                print!("{}", c);
                                io::stdout().flush().map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
            }
        }
    }
}

// Shim for other backends
pub type ReedlineInputBackend = WasmInputBackend;
pub type BasicInputBackend = WasmInputBackend;
pub type MinimalInputBackend = WasmInputBackend;
