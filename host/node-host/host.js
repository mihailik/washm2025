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
  const { startHostInstance } = require('./index');
  const inst = await startHostInstance(wasmPath);

  // Demo: if guest exports `send_eval`, call it once so we can see the end-to-end flow.
  if (inst && inst.instance && typeof inst.instance.exports.send_eval === 'function') {
    try { inst.instance.exports.send_eval(); } catch (e) { /* ignore */ }
  }

  const memory = inst.instance && inst.instance.exports && inst.instance.exports.memory;
  const enc = new TextEncoder();
  while (true) {
    try {
      const files = await fs.readdir(EVAL_DIR);
      for (const f of files) {
        if (!f.endsWith('.js')) continue;
        const cookie = f.replace(/\.js$/, '');
        try {
          const js = await fs.readFile(path.join(EVAL_DIR, f), 'utf8');
          const ctx = vm.createContext({});
          const script = new vm.Script(js);
          let result;
          try {
            result = script.runInContext(ctx, { timeout: 2000 });
          } catch (err) {
            result = { error: String(err) };
          }
          const json = JSON.stringify({ ok: true, result });
          await fs.writeFile(path.join(EVAL_DIR, cookie + '.json'), json, 'utf8');

          // Write json into guest memory and call _continue
          const alloc = inst.instance.exports.guest_alloc;
          let resultPtr = 0;
          if (typeof alloc === 'function') {
            resultPtr = alloc(json.length);
            const buf = new Uint8Array(memory.buffer, resultPtr, json.length);
            buf.set(enc.encode(json));
          } else {
            // fallback to a static scratch area at offset 8192 (unsafe)
            const fallbackPtr = 8192;
            const buf = new Uint8Array(memory.buffer, fallbackPtr, json.length);
            buf.set(enc.encode(json));
            resultPtr = fallbackPtr;
          }

          const metaPtr = typeof alloc === 'function' ? alloc(16) : 8360;
          const dv = new DataView(memory.buffer, metaPtr, 16);
          dv.setUint32(0, Number(cookie), true);
          dv.setUint32(4, 0, true);
          dv.setUint32(8, json.length, true);
          dv.setUint32(12, resultPtr, true);

          if (typeof inst.instance.exports._continue === 'function') {
            inst.instance.exports._continue(metaPtr);
          }

          await fs.unlink(path.join(EVAL_DIR, f));
        } catch (err) {
          console.error('error processing request', f, err);
        }
      }
    } catch (err) {
      // ignore
    }
    await new Promise(r=>setTimeout(r, 200));
  }
}

if (require.main === module) {
  const wasm = process.argv[2];
  if (!wasm) {
    console.error('usage: node host.js <guest.wasm>');
    process.exit(2);
  }
  run(wasm).catch((e)=>console.error(e));
}
