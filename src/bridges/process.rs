use crate::bridges::eval::{eval_blocking, WasmHostFuture};
use serde::{Deserialize, Serialize};
use std::fs;

const BRIDGE_JS: &str = include_str!("process.js");

pub fn ensure_bridge() -> Result<(), String> {
    let js = format!("{}\n//# sourceURL=bridges/process.js\n", BRIDGE_JS);
    eval_blocking(&js).map(|_| ())
}

#[derive(Serialize)]
pub struct SpawnRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize, Debug)]
pub struct ProcessStatus {
    pub exited: bool,
    pub code: i32,
    pub signal: Option<String>,
}

pub struct Child {
    pub pid: String,
}

impl Child {
    pub fn status(&self) -> Option<ProcessStatus> {
        let path = format!("/dev/host/process/{}/status", self.pid);
        if let Ok(s) = fs::read_to_string(path) {
            serde_json::from_str(&s).ok()
        } else {
            None
        }
    }

    pub async fn wait(&self) -> Result<ProcessStatus, String> {
        loop {
            if let Some(status) = self.status() {
                if status.exited {
                    return Ok(status);
                }
            }
            // Wait for a continuation from the host
            WasmHostFuture::new("null")
                .await
                .map_err(|e| e.to_string())?;
        }
    }
}

pub fn spawn(req: SpawnRequest) -> Result<Child, String> {
    let json = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    fs::write("/dev/host/process/spawn.json", json).map_err(|e| e.to_string())?;
    let pid = fs::read_to_string("/dev/host/process/spawn.json").map_err(|e| e.to_string())?;

    if pid.starts_with('{') {
        return Err(pid); // Error JSON
    }

    Ok(Child { pid })
}
