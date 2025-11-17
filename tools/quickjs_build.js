#!/usr/bin/env node
'use strict';

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

function run(cmd, args) {
  return new Promise((resolve, reject) => {
    const p = spawn(cmd, args, { stdio: 'inherit' });
    p.on('close', (code) => code === 0 ? resolve() : reject(new Error(`${cmd} ${args.join(' ')} exited ${code}`)));
  });
}

(async () => {
  try {
    const QUICKJS_DIR = process.argv[2] || path.join(process.cwd(), 'third_party', 'quickjs');
    if (!fs.existsSync(QUICKJS_DIR)) {
      console.error(`QuickJS not found at ${QUICKJS_DIR}.`);
      process.exit(1);
    }

    // Ensure emcc is in path
    try { await run('emcc', ['--version']); } catch (e) { console.error('Emscripten (emcc) is required. Install via emsdk.'); process.exit(1); }

    const srcs = ['src/quickjs.c','src/cutils.c','src/libregexp.c','src/quickjs-lib.c'].map(s => path.join(QUICKJS_DIR, s));
    const args = [].concat(srcs, ['-I.', '-O3','-s','WASM=1','-s','ASYNCIFY=1','-s','ALLOW_TABLE_GROWTH=1','-o','quickjs.wasm']);

    process.chdir(QUICKJS_DIR);
    await run('emcc', args);
    const dest = path.join(process.cwd(), '..', '..', 'host', 'quickjs-wasm.wasm');
    fs.renameSync(path.join(QUICKJS_DIR,'quickjs.wasm'), dest);
    console.log('Built quickjs wasm:', dest);
  } catch (err) {
    console.error(err);
    process.exit(1);
  }
})();
