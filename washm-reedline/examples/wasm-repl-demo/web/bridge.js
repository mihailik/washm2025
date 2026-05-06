// Wrapper around the WASM exports.
// Shared by both browser and Node environments.
//
// The host (`hostFns`) provides language-aware highlight + completion services
// that are called back into from inside the WASM `Highlighter` / `Completer`
// trait implementations. See `ts-host.js` for the TypeScript-backed default.

export const KEYS = {
    BASE: 0x20_0000,
    ENTER: 0x20_0001,
    BACKSPACE: 0x20_0002,
    TAB: 0x20_0003,
    LEFT: 0x20_0004,
    RIGHT: 0x20_0005,
    UP: 0x20_0006,
    DOWN: 0x20_0007,
    HOME: 0x20_0008,
    END: 0x20_0009,
    DELETE: 0x20_000a,
    ESC: 0x20_000b,
};

export const MODS = {
    NONE: 0,
    CTRL: 1,
    ALT: 2,
    SHIFT: 4,
};

export const RES = {
    OK: 0,
    SUBMIT: 1,
    CANCEL: 2,
};

const SEP = "\x1f";

export async function createBridge(wasmBytes, hostFns) {
    // hostFns:
    //   highlight(line: string, cursor: number) -> string (ANSI-colored)
    //   complete(line: string, cursor: number) -> { start, end, suggestions: string[] }
    if (!hostFns) {
        hostFns = {
            highlight: (line) => line,
            complete: () => ({ start: 0, end: 0, suggestions: [] }),
        };
    }

    let exports; // late-bound; populated after instantiate
    let ioPtr = 0;
    let ioCap = 0;

    const dec = new TextDecoder();
    const enc = new TextEncoder();

    const memBytes = () => new Uint8Array(exports.memory.buffer);

    const decodeIo = (len) => dec.decode(memBytes().subarray(ioPtr, ioPtr + len));
    const writeIo = (s) => {
        const b = enc.encode(s);
        const n = Math.min(b.length, ioCap);
        memBytes().set(b.subarray(0, n), ioPtr);
        return n;
    };

    const env = {
        host_highlight(in_len, cursor) {
            const text = decodeIo(in_len);
            let out;
            try {
                out = hostFns.highlight(text, cursor);
            } catch (e) {
                console.error("host_highlight error:", e);
                out = text;
            }
            return writeIo(out ?? text);
        },
        host_complete(in_len, cursor) {
            const text = decodeIo(in_len);
            let r;
            try {
                r = hostFns.complete(text, cursor);
            } catch (e) {
                console.error("host_complete error:", e);
                r = null;
            }
            if (!r || !r.suggestions || r.suggestions.length === 0) return 0;
            const start = r.start ?? cursor;
            const end = r.end ?? cursor;
            const payload = `${start},${end}${SEP}${r.suggestions.join("\n")}`;
            return writeIo(payload);
        },
    };

    const { instance } = await WebAssembly.instantiate(wasmBytes, { env });
    exports = instance.exports;
    ioPtr = exports.reedline_io_buf_ptr();
    ioCap = exports.reedline_io_buf_cap();

    const str = (ptr, len) => dec.decode(memBytes().subarray(ptr, ptr + len));

    return {
        init() {
            exports.reedline_init();
        },
        processKey(key, mods) {
            return exports.reedline_process_key(key, mods);
        },
        buffer() {
            return str(exports.reedline_buffer_ptr(), exports.reedline_buffer_len());
        },
        highlighted() {
            return str(exports.reedline_highlighted_ptr(), exports.reedline_highlighted_len());
        },
        completions() {
            const raw = str(
                exports.reedline_completions_ptr(),
                exports.reedline_completions_len(),
            );
            return raw ? raw.split("\n").filter(Boolean) : [];
        },
        cursor() {
            return exports.reedline_cursor();
        },
        lastSubmission() {
            return str(
                exports.reedline_last_submission_ptr(),
                exports.reedline_last_submission_len(),
            );
        },
    };
}
