use crate::wasm_async::eval_blocking;
use serde::Deserialize;

const RAW_ENABLE_JS: &str = include_str!("./tty/raw-enable.js");
const RAW_DISABLE_JS: &str = include_str!("./tty/raw-disable.js");
const SIZE_JS: &str = include_str!("./tty/size.js");

#[derive(Deserialize)]
struct TtyResult {
    ok: bool,
    error: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

pub fn enable_raw_mode() -> Result<(), String> {
    let res_json = eval_blocking(RAW_ENABLE_JS)?;
    // The result from eval is a JS object, which axy.js serializes to JSON.
    // Wait, axy.js does: entry.json = JSON.stringify({ ok: true, result });
    // So the result we get in Rust is `{ "ok": true, "result": { "ok": true, ... } }`.
    // We need to parse that wrapper first?
    // Let's check axy.js again.
    
    // axy.js:
    // entry.json = JSON.stringify({ ok: true, result });
    
    // So the string we read in Rust is `{"ok":true,"result":...}`.
    // But `eval_blocking` returns that string.
    
    #[derive(Deserialize)]
    struct HostResult {
        ok: bool,
        result: Option<TtyResult>,
        error: Option<String>,
    }

    let host_res: HostResult = serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;
    
    if !host_res.ok {
        return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
    }
    
    let tty_res = host_res.result.ok_or("No result from host")?;
    
    if tty_res.ok {
        Ok(())
    } else {
        Err(tty_res.error.unwrap_or_else(|| "Unknown error enabling raw mode".to_string()))
    }
}

pub fn disable_raw_mode() -> Result<(), String> {
    let res_json = eval_blocking(RAW_DISABLE_JS)?;
    
    #[derive(Deserialize)]
    struct HostResult {
        ok: bool,
        result: Option<TtyResult>,
        error: Option<String>,
    }

    let host_res: HostResult = serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;
    
    if !host_res.ok {
        return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
    }
    
    let tty_res = host_res.result.ok_or("No result from host")?;
    
    if tty_res.ok {
        Ok(())
    } else {
        Err(tty_res.error.unwrap_or_else(|| "Unknown error disabling raw mode".to_string()))
    }
}

pub fn size() -> Result<(u16, u16), String> {
    let res_json = eval_blocking(SIZE_JS)?;
    
    #[derive(Deserialize)]
    struct HostResult {
        ok: bool,
        result: Option<TtyResult>,
        error: Option<String>,
    }

    let host_res: HostResult = serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;
    
    if !host_res.ok {
        return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
    }
    
    let tty_res = host_res.result.ok_or("No result from host")?;
    
    if tty_res.ok {
        Ok((tty_res.cols.unwrap_or(80), tty_res.rows.unwrap_or(24)))
    } else {
        Err(tty_res.error.unwrap_or_else(|| "Unknown error getting size".to_string()))
    }
}

pub struct RawModeGuard;

impl RawModeGuard {
    pub fn new() -> Result<Self, String> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}
