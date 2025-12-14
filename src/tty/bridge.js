(function() {
    if (globalThis.washm_tty) return JSON.stringify({ ok: true });

    const fs = require('fs');
    let buffer = [];
    let pendingResolve = null;
    let stdin = process.stdin;
    let isRaw = false;

    try {
        if (stdin.isTTY) {
            stdin.setRawMode(true);
            isRaw = true;
        }
        stdin.resume();
        stdin.setEncoding('utf8');
        
        stdin.on('data', (chunk) => {
            if (pendingResolve) {
                const resolve = pendingResolve;
                pendingResolve = null;
                resolve(chunk);
            } else {
                buffer.push(chunk);
            }
        });
        
        stdin.on('end', () => {
             if (pendingResolve) {
                const resolve = pendingResolve;
                pendingResolve = null;
                resolve(null); // EOF
            } else {
                buffer.push(null);
            }
        });
        
    } catch (e) {
        return { ok: false, error: e.message };
    }

    globalThis.washm_tty = {
        read: () => {
            return new Promise((resolve) => {
                if (buffer.length > 0) {
                    resolve(buffer.shift());
                } else {
                    pendingResolve = resolve;
                }
            });
        },
        enableRaw: () => {
            if (stdin.isTTY) {
                stdin.setRawMode(true);
                isRaw = true;
                return { ok: true };
            }
            return { ok: false, error: "Not a TTY" };
        },
        disableRaw: () => {
            if (stdin.isTTY) {
                stdin.setRawMode(false);
                isRaw = false;
                return { ok: true };
            }
            return { ok: false, error: "Not a TTY" };
        },
        size: () => {
            if (process.stdout.isTTY) {
                return { ok: true, rows: process.stdout.rows, cols: process.stdout.columns };
            }
            return { ok: false, error: "Not a TTY" };
        }
    };

    return { ok: true };
})();
