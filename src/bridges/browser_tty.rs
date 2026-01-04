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
    let css_injection = format!(
        r#"
(function() {{
    const style = document.createElement('style');
    style.textContent = `{}`;
    document.head.appendChild(style);
    return {{ ok: true }};
}})();
//# sourceURL=bridges/xterm.css.js
"#,
        css_content.replace("`", "\\`") // Escape backticks in CSS
    );
    let _ = eval_blocking(&css_injection)?;

    // 2. Inject xterm.js
    let xterm_js = include_str!("../../node_modules/xterm/lib/xterm.js");
    let xterm_wrapper = format!(
        r#"
(function() {{
    // Execute the library code
    {}
    return {{ ok: true }};
}})();
//# sourceURL=bridges/xterm_loader.js
"#,
        xterm_js
    );
    let _ = eval_blocking(&xterm_wrapper)?;

    // 3. Inject xterm-addon-fit.js
    let fit_js = include_str!("../../node_modules/xterm-addon-fit/lib/xterm-addon-fit.js");
    let fit_wrapper = format!(
        r#"
(function() {{
    {}
    return {{ ok: true }};
}})();
//# sourceURL=bridges/xterm_fit_loader.js
"#,
        fit_js
    );
    let _ = eval_blocking(&fit_wrapper)?;

    // 4. Initialize Bridge Logic
    let bridge_js = r#"
(function() {
    if (typeof globalThis.washm_tty !== 'undefined') {
        return { ok: true };
    }

    const terminalDiv = document.createElement('div');
    terminalDiv.id = 'terminal';
    terminalDiv.style.width = '100%';
    terminalDiv.style.height = '100vh';
    terminalDiv.style.margin = '0';
    terminalDiv.style.padding = '0';
    terminalDiv.style.overflow = 'hidden';
    document.body.innerHTML = '';
    document.body.appendChild(terminalDiv);
    document.body.style.margin = '0';
    document.body.style.padding = '0';
    document.body.style.background = '#000';

    const link = document.createElement('link');
    link.rel = 'icon';
    link.href = 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><text y=".9em" font-size="90">🐚</text></svg>';
    document.head.appendChild(link);

    document.title = 'WASHM';

    const Terminal = window.Terminal;
    const FitAddon = window.FitAddon ? window.FitAddon.FitAddon : (window.FitAddon || window.xtermAddonFit);

    if (!Terminal) {
        throw new Error("Xterm.js not loaded correctly");
    }

    const term = new Terminal({
        cursorBlink: true,
        convertEol: true,
        fontFamily: 'Menlo, Monaco, "Courier New", monospace',
        fontSize: 14,
        theme: {
            background: '#000000',
            foreground: '#00ff00'
        },
        screenReaderMode: true
    });

    const fitAddon = new FitAddon ? new FitAddon() : null;
    if (fitAddon) {
        term.loadAddon(fitAddon);
    }

    term.open(terminalDiv);
    if (fitAddon) fitAddon.fit();

    window.addEventListener('resize', () => {
        if (fitAddon) fitAddon.fit();
    });

    term.focus();

    let inputBuffer = [];
    let pendingResolve = null;

    term.onData(data => {
        if (pendingResolve) {
            const resolve = pendingResolve;
            pendingResolve = null;
            resolve(data);
        } else {
            inputBuffer.push(data);
        }
    });

    if (!globalThis.washm_wasi_overrides) {
        globalThis.washm_wasi_overrides = {};
    }

    const FS_PREFIX = 'washm_fs_';
    const fsGet = (path) => sessionStorage.getItem(FS_PREFIX + path);
    const fsSet = (path, data) => sessionStorage.setItem(FS_PREFIX + path, data);
    const fsDel = (path) => sessionStorage.removeItem(FS_PREFIX + path);
    const fsList = () => {
        const files = [];
        for (let i = 0; i < sessionStorage.length; i++) {
            const key = sessionStorage.key(i);
            if (key && key.startsWith(FS_PREFIX)) {
                files.push(key.substring(FS_PREFIX.length));
            }
        }
        return files;
    };

    const openFiles = new Map();
    let nextFd = 10;

    globalThis.washm_wasi_overrides.path_open = function(dirfd, dirflags, pathPtr, pathLen, oflags, rightsBase, rightsInheriting, fdflags, fdPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        
        let content = fsGet(normPath);
        if (content === null && (oflags & 1)) {
            content = '';
            fsSet(normPath, content);
        }
        
        if (content !== null || (oflags & 1)) {
            const fd = nextFd++;
            openFiles.set(fd, { path: normPath, pos: 0, flags: oflags });
            new DataView(memory.buffer).setUint32(fdPtr, fd, true);
            return 0;
        }
        return 44; // ENOENT
    };

    globalThis.washm_wasi_overrides.fd_read = function(fd, iovsPtr, iovsLen, nreadPtr) {
        const file = openFiles.get(fd);
        if (!file) return 8; // EBADF
        
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const view = new DataView(memory.buffer);
        const content = fsGet(file.path) || '';
        const encoder = new TextEncoder();
        const bytes = encoder.encode(content);
        
        let totalRead = 0;
        for (let i = 0; i < iovsLen; i++) {
            const bufPtr = view.getUint32(iovsPtr + i * 8, true);
            const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
            const toRead = Math.min(bufLen, bytes.length - file.pos);
            if (toRead > 0) {
                new Uint8Array(memory.buffer).set(bytes.slice(file.pos, file.pos + toRead), bufPtr);
                file.pos += toRead;
                totalRead += toRead;
            }
        }
        view.setUint32(nreadPtr, totalRead, true);
        return 0;
    };

    globalThis.washm_wasi_overrides.fd_write = function(fd, iovsPtr, iovsLen, nwrittenPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const view = new DataView(memory.buffer);
        
        if (fd === 1 || fd === 2) {
            const uint8Array = new Uint8Array(memory.buffer);
            let total = 0;
            for (let i = 0; i < iovsLen; i++) {
                const ptr = view.getUint32(iovsPtr + i * 8, true);
                const len = view.getUint32(iovsPtr + i * 8 + 4, true);
                const chunk = uint8Array.slice(ptr, ptr + len);
                term.write(chunk);
                total += len;
            }
            view.setUint32(nwrittenPtr, total, true);
            return 0;
        }
        
        const file = openFiles.get(fd);
        if (!file) return 8; // EBADF
        
        let content = fsGet(file.path) || '';
        let totalWritten = 0;
        for (let i = 0; i < iovsLen; i++) {
            const bufPtr = view.getUint32(iovsPtr + i * 8, true);
            const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
            const bytes = new Uint8Array(memory.buffer, bufPtr, bufLen);
            content += new TextDecoder().decode(bytes);
            totalWritten += bufLen;
        }
        fsSet(file.path, content);
        view.setUint32(nwrittenPtr, totalWritten, true);
        return 0;
    };

    globalThis.washm_wasi_overrides.fd_close = function(fd) {
        if (openFiles.has(fd)) {
            openFiles.delete(fd);
            return 0;
        }
        if (fd <= 2 || fd === 20251219) return 0;
        return 8; // EBADF
    };

    globalThis.washm_wasi_overrides.path_unlink_file = function(dirfd, pathPtr, pathLen) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        fsDel(normPath);
        return 0;
    };

    globalThis.washm_wasi_overrides.fd_readdir = function(fd, bufPtr, bufLen, cookie, bufusedPtr) {
        const files = fsList();
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const encoder = new TextEncoder();
        let offset = 0;
        for (let i = cookie; i < files.length && offset < bufLen; i++) {
            const name = files[i];
            const nameBytes = encoder.encode(name);
            if (offset + 24 + nameBytes.length > bufLen) break;
            const view = new DataView(memory.buffer);
            view.setBigUint64(bufPtr + offset, BigInt(i + 1), true); // d_next
            view.setBigUint64(bufPtr + offset + 8, BigInt(0), true); // d_ino
            view.setUint32(bufPtr + offset + 16, nameBytes.length, true); // d_namlen
            view.setUint8(bufPtr + offset + 20, 4); // d_type (regular file)
            new Uint8Array(memory.buffer).set(nameBytes, bufPtr + offset + 24);
            offset += 24 + nameBytes.length;
        }
        new DataView(memory.buffer).setUint32(bufusedPtr, offset, true);
        return 0;
    };

    globalThis.washm_tty = {
        read: () => {
            return new Promise((resolve) => {
                if (inputBuffer.length > 0) {
                    resolve(inputBuffer.shift());
                } else {
                    pendingResolve = resolve;
                }
            });
        },
        enableRaw: () => { return { ok: true }; },
        disableRaw: () => { return { ok: true }; },
        size: () => {
            return {
                ok: true,
                rows: term.rows,
                cols: term.cols
            };
        }
    };

    return { ok: true };
})();
"#;

    let js_with_source = format!("{}\n//# sourceURL=bridges/browser_tty.js\n", bridge_js);
    
    // Use eval_blocking to execute the bridge setup
    // This returns immediately with the result
    let res_json = eval_blocking(&js_with_source)?;
    
    let host_res: HostResult = serde_json::from_str(&res_json)
        .map_err(|e| format!("JSON error: {}", e))?;
    
    if !host_res.ok {
        return Err(host_res
            .error
            .unwrap_or_else(|| "Unknown bridge init error".to_string()));
    }
    
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
