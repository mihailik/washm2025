#!/usr/bin/env node
// @ts-check

/* Node host for WASI Async interop. Implements the /dev/host/eval protocol + _continue.

   This host uses `node:wasi` and expects a WASM guest compiled for `wasm32-wasi`.
*/

const fs = require('fs/promises');
const path = require('path');
const { WASI } = require('wasi');
const { TextDecoder, TextEncoder } = require('util');

// Expose require and process globally for AsyncFunction access
global.require = require;
global.process = process;

// `/dev/host/eval` is virtual-only; no host directory mapping is allowed.

async function run() {

  let wasmPath = process.argv[2];
  if (!wasmPath) {
    wasmPath = path.resolve(__dirname, 'target/wasm32-wasip1/debug/washm.wasm');
    try {
      await fs.access(wasmPath);
    } catch (e) {
      wasmPath = path.resolve(__dirname, 'washm.wasm');
      try {
        await fs.access(wasmPath);
      } catch (e) {
        console.error('usage: node axy.js <guest.wasm>');
        process.exit(2);
        return;
      }
    }
  }

  // By spec, /dev/host/eval must not be preopened to a real host path; the host will implement an in-memory VFS overlay.
  const wasi = new WASI({
    args: process.argv,
    preopens: { [process.cwd()]: process.cwd() },
    version: 'preview1'
  });
  const wasm = await fs.readFile(wasmPath);
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
      try { writeU32(fd_out_ptr, fd); } catch (e) {}
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
      try { writeU32(nwritten_ptr, total); } catch (e) {}
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
      try { writeU32(nread_ptr, total); } catch (e) {}
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
            if (isAsync) { const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor; const fn = new AsyncFunction(jsText); result = await fn(); }
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
