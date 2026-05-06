// Universal host for the reedline WASM bridge (Browser + Node.js).
// Detects environment, loads TypeScript, and wires its language service 
// into reedline's Highlighter and Completer through host_* WASM imports.

import { createBridge, KEYS, MODS, RES } from "./bridge.js";
import { createTypeScriptHost } from "./ts-host.js";

const isNode = typeof process !== "undefined" && process.versions != null && process.versions.node != null;

async function main() {
    let term;
    let ts;
    let wasmBytes;

    if (isNode) {
        // --- Node.js Environment ---
        ts = (await import("typescript")).default || (await import("typescript"));
        const fs = await import("fs");
        wasmBytes = fs.readFileSync(new URL("./reedline_wasm_demo.wasm", import.meta.url));

        term = {
            write: (s) => process.stdout.write(s),
            clear: () => process.stdout.write("\x1b[2J\x1b[3J\x1b[H"),
            onData: (cb) => {
                if (process.stdin.isTTY) process.stdin.setRawMode(true);
                process.stdin.resume();
                process.stdin.setEncoding("utf8");
                process.stdin.on("data", cb);
            },
            exit: () => process.exit(0)
        };
    } else {
        // --- Browser Environment ---
        const tsModule = await import("https://esm.sh/typescript@5.5.4");
        ts = tsModule.default ?? tsModule;
        wasmBytes = await (await fetch("./reedline_wasm_demo.wasm")).arrayBuffer();

        const xterm = new Terminal({
            cursorBlink: true,
            fontFamily: '"Cascadia Code","Fira Code",Menlo,Consolas,monospace',
            fontSize: 14,
            theme: { background: "#111", foreground: "#ddd", cursor: "#7af" },
            convertEol: true,
        });
        xterm.open(document.getElementById("term"));
        xterm.focus();

        term = {
            write: (s) => xterm.write(s),
            clear: () => xterm.clear(),
            onData: (cb) => xterm.onData(cb),
            exit: () => {} // No-op in browser
        };
    }

    const tsHost = createTypeScriptHost(ts);
    globalThis.tsHost = tsHost; // expose for ad-hoc poking from devtools/Node REPL

    const bridge = await createBridge(wasmBytes, tsHost);
    globalThis.bridge = bridge;
    bridge.init();

    const PROMPT = ">> ";
    const promptShownAtCol = 3; // visible prompt width

    term.write(`TypeScript ${ts.version} ready. Try: \x1b[36mconsole.\x1b[0m, \x1b[36mMath.\x1b[0m, \x1b[36mlet x = 1\x1b[0m\r\n`);
    if (isNode) term.write("Type \x1b[36mexit\x1b[0m or press Ctrl+D to quit.\r\n");
    writePrompt();

    term.onData((data) => {
        for (const ev of decode(data)) {
            // In raw mode Node.js won't automatically exit on Ctrl+C/Ctrl+D gracefully, so we handle it:
            if (isNode && ((ev.key === 0x63 && ev.mods === MODS.CTRL) || (ev.key === 0x64 && ev.mods === MODS.CTRL))) {
                term.write("\r\n");
                term.exit();
            }

            const res = bridge.processKey(ev.key, ev.mods);
            if (res === RES.SUBMIT) {
                const submitted = bridge.lastSubmission();
                term.write("\r\n");
                if (submitted.trim() === "help") {
                    term.write(
                        "\x1b[36mhelp\x1b[0m: type a TS expression and Enter. Tab cycles completions. Up/Down for history.\r\n",
                    );
                } else if (submitted.trim() === "clear") {
                    term.clear();
                } else if (isNode && submitted.trim() === "exit") {
                    term.exit();
                } else if (submitted.trim() !== "") {
                    term.write(`\x1b[90mecho:\x1b[0m ${submitted}\r\n`);
                }
                writePrompt();
            } else if (res === RES.CANCEL) {
                term.write("^C\r\n");
                writePrompt();
            } else {
                if (ev.key === KEYS.TAB) {
                    applyTabCompletion();
                }
                redraw();
            }
        }
    });

    function applyTabCompletion() {
        const suggs = bridge.completions();
        if (suggs.length === 0) return;
        const line = bridge.buffer();
        const cursor = bridge.cursor();
        // Compute the replacement span identically to ts-host.
        let start = cursor;
        while (start > 0 && /[A-Za-z0-9_$]/.test(line[start - 1])) start--;
        // Replace [start, cursor) with the first suggestion via the existing key API:
        //   1) backspace `cursor - start` times
        //   2) type the suggestion characters
        const removeCount = cursor - start;
        for (let i = 0; i < removeCount; i++) bridge.processKey(KEYS.BACKSPACE, 0);
        for (const c of suggs[0]) bridge.processKey(c.codePointAt(0), 0);
    }

    function writePrompt() {
        term.write(PROMPT);
        redraw();
    }

    function redraw() {
        const highlighted = bridge.highlighted();
        const cursor = bridge.cursor();
        term.write("\r\x1b[K" + PROMPT + highlighted);
        const col = promptShownAtCol + cursor + 1; // 1-based
        term.write(`\r\x1b[${col}G`);
    }
}

// ---------------------------------------------------------------------------
// Decode raw xterm.js / Node.js input bytes into our (key, mods) tuples.
// ---------------------------------------------------------------------------
function decode(s) {
    const out = [];
    for (let i = 0; i < s.length; i++) {
        const c = s[i];
        const code = s.charCodeAt(i);

        if (c === "\x1b" && s[i + 1] === "[") {
            const seq = s.slice(i + 2);
            const m = /^([A-D])/.exec(seq) || /^[0-9;]*([HF~])/.exec(seq);
            if (m) {
                const tail = m[1];
                const consumed = 2 + m[0].length;
                i += consumed - 1;
                switch (tail) {
                    case "A": out.push({ key: KEYS.UP, mods: 0 }); break;
                    case "B": out.push({ key: KEYS.DOWN, mods: 0 }); break;
                    case "C": out.push({ key: KEYS.RIGHT, mods: 0 }); break;
                    case "D": out.push({ key: KEYS.LEFT, mods: 0 }); break;
                    case "H": out.push({ key: KEYS.HOME, mods: 0 }); break;
                    case "F": out.push({ key: KEYS.END, mods: 0 }); break;
                    case "~": {
                        if (m[0].startsWith("3")) out.push({ key: KEYS.DELETE, mods: 0 });
                        break;
                    }
                }
                continue;
            }
        }
        if (c === "\x1b") { out.push({ key: KEYS.ESC, mods: 0 }); continue; }
        if (c === "\r" || c === "\n") { out.push({ key: KEYS.ENTER, mods: 0 }); continue; }
        if (code === 0x7f || code === 0x08) { out.push({ key: KEYS.BACKSPACE, mods: 0 }); continue; }
        if (c === "\t") { out.push({ key: KEYS.TAB, mods: 0 }); continue; }
        if (code >= 1 && code <= 26) {
            out.push({ key: 0x60 + code, mods: MODS.CTRL });
            continue;
        }
        out.push({ key: c.codePointAt(0), mods: 0 });
        if (c.codePointAt(0) > 0xffff) i++;
    }
    return out;
}

main().catch(err => {
    console.error(err);
    if (isNode) process.exit(1);
});
