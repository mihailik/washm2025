// Headless smoke test driving the TypeScript-backed bridge end-to-end.
// Run with: node test.js  (requires `npm install` in this folder).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { createBridge, KEYS, MODS, RES } from "./bridge.js";
import { createTypeScriptHost } from "./ts-host.js";
import ts from "typescript";

const __dirname = dirname(fileURLToPath(import.meta.url));
const wasmPath = resolve(__dirname, "reedline_wasm_demo.wasm");
const bytes = readFileSync(wasmPath);
const tsHost = createTypeScriptHost(ts);
const bridge = await createBridge(
    bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    tsHost,
);

let pass = 0,
    fail = 0;
const eq = (label, got, want) => {
    if (got === want) {
        pass++;
        console.log(`  ok  ${label}`);
    } else {
        fail++;
        console.log(`FAIL ${label}: got ${JSON.stringify(got)} want ${JSON.stringify(want)}`);
    }
};
const truthy = (label, got) => eq(label, !!got, true);

// --- editor basics ---------------------------------------------------------
bridge.init();
eq("empty buffer", bridge.buffer(), "");
eq("cursor=0", bridge.cursor(), 0);

for (const c of "hello") bridge.processKey(c.codePointAt(0), 0);
eq("type hello", bridge.buffer(), "hello");
eq("cursor at end", bridge.cursor(), 5);

bridge.processKey("a".charCodeAt(0), MODS.CTRL);
eq("ctrl-a moves home", bridge.cursor(), 0);
bridge.processKey("e".charCodeAt(0), MODS.CTRL);
eq("ctrl-e moves end", bridge.cursor(), 5);

bridge.processKey(KEYS.BACKSPACE, 0);
eq("backspace removes 'o'", bridge.buffer(), "hell");

const r = bridge.processKey(KEYS.ENTER, 0);
eq("enter returns SUBMIT", r, RES.SUBMIT);
eq("last submission is 'hell'", bridge.lastSubmission(), "hell");
eq("buffer cleared after submit", bridge.buffer(), "");

// --- history ---------------------------------------------------------------
for (const c of "world") bridge.processKey(c.codePointAt(0), 0);
bridge.processKey(KEYS.ENTER, 0);
bridge.processKey(KEYS.UP, 0);
eq("up restores last submission", bridge.buffer(), "world");
bridge.processKey(KEYS.UP, 0);
eq("up at oldest stays", bridge.buffer(), "hell");
bridge.processKey(KEYS.DOWN, 0);
eq("down moves forward", bridge.buffer(), "world");
bridge.processKey(KEYS.DOWN, 0);
eq("down past newest is empty", bridge.buffer(), "");

bridge.processKey("x".charCodeAt(0), 0);
const cancel = bridge.processKey("c".charCodeAt(0), MODS.CTRL);
eq("ctrl-c returns CANCEL", cancel, RES.CANCEL);

// --- TypeScript-driven highlighter ----------------------------------------
bridge.init();
for (const c of "let x = 1") bridge.processKey(c.codePointAt(0), 0);
const highlighted = bridge.highlighted();
console.log(`\nHighlighted "let x = 1":`);
console.log("  " + JSON.stringify(highlighted));
truthy("highlighter emits ANSI escapes", highlighted.includes("\x1b["));
truthy("highlighter colors the 'let' keyword (magenta)", highlighted.includes("\x1b[35mlet"));
truthy("highlighter colors the '1' literal (yellow)", highlighted.includes("\x1b[33m1"));
truthy("highlighter preserves text", highlighted.includes("let") && highlighted.includes("x") && highlighted.includes("1"));

bridge.init();
for (const c of '"hi" + 42') bridge.processKey(c.codePointAt(0), 0);
const hl2 = bridge.highlighted();
console.log(`\nHighlighted '"hi" + 42':`);
console.log("  " + JSON.stringify(hl2));
truthy("string literal colored green", hl2.includes('\x1b[32m"hi"'));
truthy("number literal colored yellow", hl2.includes("\x1b[33m42"));

// --- TypeScript-driven completer ------------------------------------------
bridge.init();
for (const c of "console.l") bridge.processKey(c.codePointAt(0), 0);
const compsConsole = bridge.completions();
console.log(`\nCompletions for "console.l":`, compsConsole);
truthy("console.l yields suggestions", compsConsole.length > 0);
truthy("'log' is suggested for console.l", compsConsole.includes("log"));

bridge.init();
for (const c of "Math.s") bridge.processKey(c.codePointAt(0), 0);
const compsMath = bridge.completions();
console.log(`\nCompletions for "Math.s":`, compsMath);
truthy("Math.s yields suggestions", compsMath.length > 0);
truthy("'sqrt' or 'sin' present in Math.s suggestions", compsMath.includes("sqrt") || compsMath.includes("sin"));

bridge.init();
for (const c of "co") bridge.processKey(c.codePointAt(0), 0);
const compsCo = bridge.completions();
console.log(`\nCompletions for "co":`, compsCo);
truthy("'co' yields global suggestions", compsCo.length > 0);
truthy("'console' present in 'co' suggestions", compsCo.includes("console"));

// --- Tab integration in editor: replace the prefix with the first suggestion -
bridge.init();
for (const c of "Math.P") bridge.processKey(c.codePointAt(0), 0);
const compsMP = bridge.completions();
console.log(`\nCompletions for "Math.P":`, compsMP);
if (compsMP.length > 0) {
    // Simulate what the browser app does on Tab.
    const cursor = bridge.cursor();
    let start = cursor;
    const line = bridge.buffer();
    while (start > 0 && /[A-Za-z0-9_$]/.test(line[start - 1])) start--;
    for (let i = 0; i < cursor - start; i++) bridge.processKey(KEYS.BACKSPACE, 0);
    for (const c of compsMP[0]) bridge.processKey(c.codePointAt(0), 0);
    console.log(`  buffer after tab-complete: ${JSON.stringify(bridge.buffer())}`);
    truthy("buffer ends with first completion after tab", bridge.buffer().endsWith(compsMP[0]));
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail === 0 ? 0 : 1);
