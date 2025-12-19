#!/usr/bin/env -S node --no-warnings
const fs = require('fs');
const { WASI } = require('wasi');
const argv = process.argv;

async function main() {
  const wasmPath = './washm.wasm';
  const wasmBuffer = fs.readFileSync(wasmPath);

  const wasi = new WASI({
    args: ['washm.js', ...argv.slice(2)],
    env: process.env,
    preopens: (function() {
      if (process.platform !== 'win32') return { '/': '/' };
      const preopens = {};
      try {
        const out = require('child_process').execFileSync(
          'powershell',
          ['-NoProfile', '-Command', 'Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Root'],
          { encoding: 'utf8', timeout: 1500 }
        );
        out.split(/\r?\n/)
          .map(s => s.trim())
          .filter(Boolean)
          .forEach(root => {
            const path = root.endsWith('\\') ? root : root + '\\';
            const letter = path[0].toUpperCase();
            preopens[`/${letter}:/`] = path;
          });
      } catch (e) {
        // Fallback: use roots from current directory and user's home
        const pathModule = require('path');
        const os = require('os');
        const cwdRoot = pathModule.parse(process.cwd()).root;
        const homeRoot = pathModule.parse(os.homedir()).root;
        [cwdRoot, homeRoot].filter(Boolean).forEach(r => {
          const rootPath = r.endsWith('\\') ? r : r + '\\';
          const letter = rootPath[0].toUpperCase();
          preopens[`/${letter}:/`] = rootPath;
        });
      }
      return preopens;
    })(),
    version: 'preview1'
  });

  const wasiSnapshot = {};
  for (const key of Object.keys(wasi.wasiImport)) {
    wasiSnapshot[key] = function () {
      return wasiSnapshot[key].actual.apply(this, arguments);
    };
    wasiSnapshot[key].actual = wasi.wasiImport[key];
  }

  let collectEvalJS = "";
  let instance;
  const importObj = {
    wasi_snapshot_preview1: wasiSnapshot
  };

  // Special handling for bootstrap FD 20251219
  const origFdWrite = wasiSnapshot.fd_write.actual;
  wasiSnapshot.fd_write.actual = (fd, iovs, iovsLen, nwritten) => {
    if (fd === 20251219) {
      const memory = new Uint8Array(instance.exports.memory.buffer);
      const view = new DataView(memory.buffer);
      let total = 0;
      for (let i = 0; i < iovsLen; i++) {
        const ptr = view.getUint32(iovs + i * 8, true);
        const len = view.getUint32(iovs + i * 8 + 4, true);
        const chunk = memory.slice(ptr, ptr + len);
        collectEvalJS += new TextDecoder().decode(chunk);
        total += len;
      }
      view.setUint32(nwritten, total, true);
      return 0;
    }
    return origFdWrite(fd, iovs, iovsLen, nwritten);
  };

  const origFdClose = wasiSnapshot.fd_close.actual;
  wasiSnapshot.fd_close.actual = (fd) => {
    if (fd === 20251219) {
      const js = collectEvalJS;
      collectEvalJS = '';
      eval(js);
      return 0;
    }
    return origFdClose(fd);
  };

  const result = await WebAssembly.instantiate(wasmBuffer, importObj);
  instance = result.instance;

  globalThis.__washm_bootstrap = {
    wasi,
    importObj,
    wasiSnapshot,
    instance,
    require,
  };

  wasi.start(instance);
}

main();
