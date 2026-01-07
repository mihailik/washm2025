use crate::bridges::eval::eval_blocking;
use serde::Deserialize;

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

/// Ensure the browser TTY bridge is initialized
pub fn ensure_bridge() -> Result<(), String> {
    // 1. Inject CSS
    let css_content = include_str!("../../node_modules/xterm/css/xterm.css");
    // Append a CSS source comment so devtools can associate the injected stylesheet
    // with the original file path and JSON-escape the whole string to avoid any
    // delimiter or character issues when embedding it in JS.
    let css_with_source = format!(
        "{}\n/*# sourceURL=washm://src/node_modules/xterm/css/xterm.css */",
        css_content
    );
    let css_json = serde_json::to_string(&css_with_source).unwrap();
    let css_injection = format!(
        r#"
(function() {{
    const style = document.createElement('style');
    style.textContent = {};
    document.head.appendChild(style);
}})();
//# sourceURL=washm://src/bridges/xterm.css.js
"#,
        css_json
    );
    crate::bootstrap::eval_fire_and_forget(&css_injection);

    // 2. Inject xterm.js
    let xterm_js = include_str!("../../node_modules/xterm/lib/xterm.js");
    // Evaluate vendor code as-is: strip embedded sourceMappingURL comments and append a meaningful sourceURL
    let xterm_clean = xterm_js
        .lines()
        .filter(|l| !l.contains("sourceMappingURL"))
        .collect::<Vec<_>>()
        .join("\n");
    let xterm_source = "node_modules/xterm/lib/xterm.js";
    let xterm_wrapper = format!(
        "{}\n//# sourceURL=washm://src/{}\n",
        xterm_clean, xterm_source
    );
    crate::bootstrap::eval_fire_and_forget(&xterm_wrapper);

    // 3. Inject xterm-addon-fit.js
    let fit_js = include_str!("../../node_modules/xterm-addon-fit/lib/xterm-addon-fit.js");
    // Strip embedded sourceMappingURL comments (they produce malformed wrappers) and evaluate the file as-is
    let fit_clean = fit_js
        .lines()
        .filter(|l| !l.contains("sourceMappingURL"))
        .collect::<Vec<_>>()
        .join("\n");
    let fit_source = "node_modules/xterm-addon-fit/lib/xterm-addon-fit.js";
    let fit_wrapper = format!("{}\n//# sourceURL=washm://src/{}\n", fit_clean, fit_source);
    crate::bootstrap::eval_fire_and_forget(&fit_wrapper);

    // 4. Initialize Bridge Logic
    let bridge_js = include_str!("browser_tty.js");

    let js_with_source = format!(
        "{}\n//# sourceURL=washm://src/bridges/browser_tty.js\n",
        bridge_js
    );

    // Use eval_blocking to execute the bridge setup
    // This returns immediately with the result
    crate::bootstrap::eval_fire_and_forget(&js_with_source);

    Ok(())
}

pub fn enable_raw_mode() -> Result<(), String> {
    ensure_bridge()?;
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
