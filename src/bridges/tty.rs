use crate::bridges::eval::eval_blocking;
use crate::platform;
use serde::Deserialize;

const BRIDGE_JS: &str = include_str!("tty.js");

#[derive(Deserialize)]
struct TtyResult {
    ok: bool,
    error: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

#[derive(Deserialize)]
struct HostResult {
    ok: bool,
    result: Option<TtyResult>,
    error: Option<String>,
}

pub fn ensure_bridge() -> Result<(), String> {
    // Detect platform and delegate to appropriate bridge
    if platform::is_browser() {
        crate::bridges::browser_tty::ensure_bridge()
    } else {
        // Node.js path
        let js = format!("{}\n//# sourceURL=washm:/bridges/tty.js\n", BRIDGE_JS);
        crate::bootstrap::eval_fire_and_forget(&js);
        Ok(())
    }
}

pub fn enable_raw_mode() -> Result<(), String> {
    ensure_bridge()?;

    if platform::is_browser() {
        return crate::bridges::browser_tty::enable_raw_mode();
    }

    let res_json = eval_blocking("washm_tty.enableRaw()")?;
    let host_res: HostResult =
        serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;

    if !host_res.ok {
        return Err(host_res
            .error
            .unwrap_or_else(|| "Unknown host error".to_string()));
    }

    let tty_res = host_res.result.ok_or("No result from host")?;
    if tty_res.ok {
        Ok(())
    } else {
        Err(tty_res
            .error
            .unwrap_or_else(|| "Unknown error enabling raw mode".to_string()))
    }
}

pub fn disable_raw_mode() -> Result<(), String> {
    ensure_bridge()?;

    if platform::is_browser() {
        return crate::bridges::browser_tty::disable_raw_mode();
    }

    let res_json = eval_blocking("washm_tty.disableRaw()")?;
    let host_res: HostResult =
        serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;

    if !host_res.ok {
        return Err(host_res
            .error
            .unwrap_or_else(|| "Unknown host error".to_string()));
    }

    let tty_res = host_res.result.ok_or("No result from host")?;
    if tty_res.ok {
        Ok(())
    } else {
        Err(tty_res
            .error
            .unwrap_or_else(|| "Unknown error disabling raw mode".to_string()))
    }
}

pub fn size() -> Result<(u16, u16), String> {
    ensure_bridge()?;

    if platform::is_browser() {
        return crate::bridges::browser_tty::size();
    }

    let res_json = eval_blocking("washm_tty.size()")?;
    let host_res: HostResult =
        serde_json::from_str(&res_json).map_err(|e| format!("JSON error: {}", e))?;

    if !host_res.ok {
        return Err(host_res
            .error
            .unwrap_or_else(|| "Unknown host error".to_string()));
    }

    let tty_res = host_res.result.ok_or("No result from host")?;
    if tty_res.ok {
        Ok((tty_res.cols.unwrap_or(80), tty_res.rows.unwrap_or(24)))
    } else {
        Err(tty_res
            .error
            .unwrap_or_else(|| "Unknown error getting size".to_string()))
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
