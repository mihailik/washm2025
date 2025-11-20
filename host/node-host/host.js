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

  const wasi = new WASI({
    args: ['washm'],
    preopens: {
      '/dev/host/eval': EVAL_DIR,
      '.': process.cwd()
    },
    version: 'preview1'
  });
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

  // Background async eval loop
  let stopped = false;
  async function evalLoop() {
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

            // Notify WASM that result is ready - just pass cookie ID
            if (typeof instance.exports._continue === 'function') {
              instance.exports._continue(Number(cookie));
            }

            await fs.unlink(path.join(EVAL_DIR, f));
          } catch (err) {
            console.error('[Host] Error processing request', f, err);
          }
        }
      } catch (err) {
        // ignore readdir errors
      }
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  // Start the passive eval loop
  evalLoop();
  
  // WASM will handle stdin and REPL - host is now passive
  console.log('[Host] Starting passive mode - WASM owns all logic');
}

if (require.main === module) {
  const wasm = process.argv[2];
  if (!wasm) {
    console.error('usage: node host.js <guest.wasm>');
    process.exit(2);
  }
  run(wasm).catch((e) => console.error(e));
}
