#!/usr/bin/env node
// @ts-check

/* Node host for WASI Async interop. Implements the /dev/host/eval protocol + _continue.

   This host uses `node:wasi` and expects a WASM guest compiled for `wasm32-wasi`.
*/

async function run() {

  const fs = require('fs/promises');
  const path = require('path');
  // NOTE: `require('wasi')` triggers a Node experimental warning when loaded.
  // We want to be able to suppress that warning early if the user didn't pass
  // `--verbose`. We therefore delay requiring `wasi` until after we've examined
  // command-line args and temporarily patched `process.emitWarning`.
  const { TextDecoder, TextEncoder } = require('util');

  // support a `--check` flag to print module summary before instantiating
  const cliArgs = process.argv.slice(2);
  const isVerbose = cliArgs.includes("--verbose");

  // If the user didn't pass `--verbose`, temporarily override
  // `process.emitWarning` to quietly drop ExperimentalWarning messages while
  // we require and initialize the `wasi` module. We restore the original
  // implementation immediately afterwards so other relevant warnings still
  // surface.
  let _origEmitWarning = null;
  let _didOverride = false;
  if (!isVerbose && typeof process !== 'undefined' && typeof process.emitWarning === 'function') {
    _origEmitWarning = process.emitWarning;
    process.emitWarning = function (warning, ...rest) {
      try {
        const name = warning && typeof warning === 'object' && warning.name ? warning.name : undefined;
        const msg = typeof warning === 'string' ? warning : (warning && warning.message) || '';
        if ((name && name === 'ExperimentalWarning') || (msg && /WASI is an experimental feature/i.test(msg))) {
          // Drop this specific Node experimental warning silently.
          return;
        }
      } catch (e) {
        // Fall through to the original handler on failure.
      }
      return _origEmitWarning.call(process, warning, ...rest);
    };
    _didOverride = true;
  }
  // detect wasm path as the first non-flag argument; otherwise we'll use defaults later
  let wasmPath = cliArgs.find(a => !a.startsWith('-'));
  // If argv[2] is missing or looks like a flag (starts with -), assume it's not the wasm file
  if (!wasmPath || wasmPath.startsWith('-')) {
    const viablePaths = ['.', 'target/wasm32-wasip1/release', 'target/wasm32-wasip1/debug'];
    for (let i = 0; i < viablePaths.length; i++) {
      const p = viablePaths[i];
      const candidate = path.resolve(__dirname, p, 'washm.wasm');
      if (i === viablePaths.length - 1) {
        // Last candidate, if not found, we'll error out later
        await fs.access(candidate);
        wasmPath = candidate;
      }

      try {
        await fs.access(candidate);
        wasmPath = candidate;
        break;
      } catch (e) {
        // continue
      }
    }
  }

  let guestArgs = [];
  if (wasmPath) {
    const wasmIndex = cliArgs.indexOf(wasmPath);
    const trailing = wasmIndex >= 0 ? cliArgs.slice(wasmIndex + 1) : cliArgs;
    guestArgs = [path.basename(wasmPath), ...trailing];
  } else {
    // No wasm path provided, leave guestArgs empty
    guestArgs = [];
  }
  const wasiArgs = guestArgs;
  let WASI = require('wasi').WASI;
  if (_didOverride && _origEmitWarning) {
    // restore original `process.emitWarning` now that `wasi` has been required
    process.emitWarning = _origEmitWarning;
  }

  const wasi = new WASI({
    args: wasiArgs,
    preopens: { [process.cwd()]: process.cwd() },
    version: 'preview1'
  });
  const wasm = await fs.readFile(wasmPath);
  // If check flag was passed, print a summary before continuing
  const module = await WebAssembly.compile(wasm);
  const importObj = { wasi_snapshot_preview1: wasi.wasiImport };

  // Setup virtual vfs
  const evalVfs = new Map();
  const fdToVfs = new Map();
  let nextFakeFd = 1000;
  const memRef = { memory: null };
  const orig = wasi.wasiImport;
  const wrapped = { ...orig };

  function readStringFromMemory(ptr, len) {
    const mem = memRef.memory;
    if (!mem) return '';
    return new TextDecoder('utf-8').decode(new Uint8Array(mem.buffer, ptr, len));
  }
  function writeU32(ptr, val) {
    new DataView(memRef.memory.buffer).setUint32(ptr, val, true);
  }

  wrapped.path_open = function (dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr) {
    const p = readStringFromMemory(path_ptr, path_len);
    if (p.startsWith('/dev/host/eval')) {
      const fd = nextFakeFd++;
      fdToVfs.set(fd, { path: p, pos: 0, mode: 'rw' });
      if (!evalVfs.has(p)) evalVfs.set(p, { js: '', json: null });
      try { writeU32(fd_out_ptr, fd); } catch (e) { }
      return 0;
    }
    return orig.path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };
  wrapped.fd_write = function (fd, iovs_ptr, iovs_len, nwritten_ptr) {
    if (fdToVfs.has(fd)) {
      const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      for (let i = 0; i < iovs_len; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const bytes = new Uint8Array(mem.buffer, bufPtr, bufLen);
        const text = new TextDecoder('utf-8').decode(bytes);
        const entry = evalVfs.get(fdToVfs.get(fd).path) || { js: '', json: null };
        entry.js += text; evalVfs.set(fdToVfs.get(fd).path, entry);
        total += bufLen;
      }
      try { writeU32(nwritten_ptr, total); } catch (e) { }
      return 0;
    }
    return orig.fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr);
  };
  wrapped.fd_read = function (fd, iovs_ptr, iovs_len, nread_ptr) {
    if (fdToVfs.has(fd)) {
      const entry = evalVfs.get(fdToVfs.get(fd).path) || { js: '', json: null };
      const readBuf = entry.json === null ? '' : entry.json; const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      for (let i = 0; i < iovs_len && total < readBuf.length; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const toWrite = Math.min(bufLen, readBuf.length - total);
        const bytes = new TextEncoder().encode(readBuf.slice(total, total + toWrite));
        new Uint8Array(mem.buffer, bufPtr, toWrite).set(bytes);
        total += toWrite;
      }
      try { writeU32(nread_ptr, total); } catch (e) { }
      return 0;
    }
    return orig.fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  };
  wrapped.fd_close = function (fd) {
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd); fdToVfs.delete(fd);
      if (info.path.endsWith('.js')) {
        const cookie = path.basename(info.path, '.js'); const entry = evalVfs.get(info.path) || { js: '', json: null };
        const jsText = entry.js;
        (async () => {
          let result;
          try {
            let isAsync = false;
            try { new Function(jsText); } catch (e) { if (e instanceof SyntaxError) isAsync = true; else throw e; }
            if (isAsync) { const AsyncFunction = Object.getPrototypeOf(async function () { }).constructor; const fn = new AsyncFunction(jsText); result = await fn(); }
            else result = eval(jsText);
            if (result instanceof Promise) result = await result;
          } catch (err) { result = { error: String(err) }; }
          entry.json = JSON.stringify({ ok: true, result }); evalVfs.set(`/dev/host/eval/${cookie}.json`, entry);
          if (typeof instance.exports._continue === 'function') instance.exports._continue(Number(cookie));
        })();
      }
      return 0;
    }
    return orig.fd_close(fd);
  };

  importObj.wasi_snapshot_preview1 = wrapped;

  const instance = await WebAssembly.instantiate(module, importObj);
  memRef.memory = instance.exports.memory;

  // Reactor initialization
  // wasi.initialize(instance) will call _initialize export if it exists.
  try {
    wasi.initialize(instance);
  } catch (e) {
    // Ignore if already initialized or if it fails safely (e.g. no _initialize)
    // But for random_get to work, wasi must be initialized.
  }
}

if (require.main === module) {
  run();
}
