#!/usr/bin/env node
'use strict';

/* Node host for WASI Async interop. Implements the /dev/host/eval protocol + _continue.

   This host uses `node:wasi` and expects a WASM guest compiled for `wasm32-wasi`.
*/

const fs = require('fs/promises');
const fsSync = require('fs');
const path = require('path');
const vm = require('vm');
const { WASI } = require('wasi');

const EVAL_DIR = path.resolve(process.cwd(), 'host_eval');
async function ensureEvalDir() {
  if (!fsSync.existsSync(EVAL_DIR)) {
    await fs.mkdir(EVAL_DIR, { recursive: true });
  }
}

async function run(wasmPath) {
  await ensureEvalDir();

  const wasi = new WASI({ preopens: { '/dev/host/eval': EVAL_DIR }, version: 'preview1' });
  const wasm = await fs.readFile(wasmPath);
  const module = await WebAssembly.compile(wasm);
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });

  // Reactor initialization
  // wasi.initialize(instance) will call _initialize export if it exists.
  try {
    wasi.initialize(instance);
  } catch (e) {
    // Ignore if already initialized or if it fails safely (e.g. no _initialize)
    // But for random_get to work, wasi must be initialized.
  }

  const memory = instance.exports.memory;
  const enc = new TextEncoder();
  const alloc = instance.exports.guest_alloc;

  // Input Loop
  const readline = require('readline');
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: 'axy> '
  });

  rl.prompt();

  rl.on('line', (line) => {
    const cmd = line.trim();
    if (!cmd) {
      rl.prompt();
      return;
    }

    if (typeof instance.exports.axy_submit !== 'function') {
      console.error('Error: axy_submit export not found in WASM module.');
      process.exit(1);
    }

    // Allocate and write string to guest memory
    const buf = enc.encode(cmd);
    let ptr = 0;
    if (typeof alloc === 'function') {
      ptr = alloc(buf.length);
    } else {
      // Fallback (unsafe)
      ptr = 8192;
    }

    const memBuf = new Uint8Array(memory.buffer, ptr, buf.length);
    memBuf.set(buf);

    // Call Reactor
    try {
      const exitCode = instance.exports.axy_submit(ptr, buf.length);
      // console.log(`[Host] Command exited with: ${exitCode}`);
    } catch (e) {
      console.error('[Host] Exception in axy_submit:', e);
    }

    rl.prompt();
  }).on('close', () => {
    process.exit(0);
  });
}

if (require.main === module) {
  const wasm = process.argv[2];
  if (!wasm) {
    console.error('usage: node host.js <guest.wasm>');
    process.exit(2);
  }
  run(wasm).catch((e) => console.error(e));
}
