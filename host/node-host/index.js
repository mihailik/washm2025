#!/usr/bin/env node
'use strict';

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

async function startHostInstance(wasmPath) {
  await ensureEvalDir();
  const wasi = new WASI({ preopens: { '/dev/host/eval': EVAL_DIR }, version: 'preview1' });
  const wasm = await fs.readFile(wasmPath);
  const module = await WebAssembly.compile(wasm);
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });

  try { wasi.start(instance); } catch (e) { /* ignore */ }

  const memory = instance.exports.memory;
  const enc = new TextEncoder();

  let stopped = false;
  async function runLoop() {
    while (!stopped) {
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

            const alloc = instance.exports.guest_alloc;
            let resultPtr = 0;
            if (typeof alloc === 'function') {
              resultPtr = alloc(json.length);
              const buf = new Uint8Array(memory.buffer, resultPtr, json.length);
              buf.set(enc.encode(json));
            } else {
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

            if (typeof instance.exports._continue === 'function') {
              instance.exports._continue(metaPtr);
            }

            await fs.unlink(path.join(EVAL_DIR, f));
          } catch (err) {
            console.error('error processing request', f, err);
          }
        }
      } catch (err) {
        // ignore
      }
      await new Promise((r) => setTimeout(r, 200));
    }
  }

  runLoop();

  return {
    instance,
    stop: async () => { stopped = true; }
  };
}

module.exports = { startHostInstance };
