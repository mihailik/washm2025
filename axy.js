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
  // Check if the detected path actually exists as a file
  if (wasmPath) {
    try {
      await fs.access(wasmPath);
    } catch (e) {
      // Path doesn't exist, treat it as a guest argument instead
      wasmPath = null;
    }
  }
  
  // If argv[2] is missing or looks like a flag (starts with -), or doesn't exist, use defaults
  if (!wasmPath) {
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
    // Check if wasmPath was explicitly provided in cliArgs
    const wasmIndex = cliArgs.indexOf(wasmPath);
    if (wasmIndex >= 0) {
      // WASM path was explicit, include everything after it
      guestArgs = [path.basename(wasmPath), ...cliArgs.slice(wasmIndex + 1)];
    } else {
      // WASM path was auto-detected, include all cliArgs as arguments
      guestArgs = [path.basename(wasmPath), ...cliArgs];
    }
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
    preopens: { '.': process.cwd() },
    version: 'preview1'
  });
  if (isVerbose) console.error("Loading WASM...");
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
    // console.error("path_open:", p);
    if (p.includes('dev/host/eval')) {
      // console.error("Opening eval path:", p);
      const fd = nextFakeFd++;
      fdToVfs.set(fd, { path: p, pos: 0, mode: 'rw' });
      if (!evalVfs.has(p)) evalVfs.set(p, { js: '', json: null });
      try { writeU32(fd_out_ptr, fd); } catch (e) { }
      return 0;
    }
    return orig.path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };
  wrapped.fd_write = function (fd, iovs_ptr, iovs_len, nwritten_ptr) {
    // console.error("fd_write:", fd);
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
      // console.error("fd_read:", fd);
      const info = fdToVfs.get(fd);
      const path = info.path;
      // console.error("fd_read: path:", path);
      const entry = evalVfs.get(path) || { js: '', json: null };
      // console.error("fd_read: entry:", entry);
      const jsonStr = entry.json === null ? '' : entry.json;
      const bytes = new TextEncoder().encode(jsonStr);
      
      const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      let currentPos = info.pos;

      for (let i = 0; i < iovs_len && currentPos < bytes.length; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const remaining = bytes.length - currentPos;
        const toWrite = Math.min(bufLen, remaining);
        
        const chunk = bytes.subarray(currentPos, currentPos + toWrite);
        new Uint8Array(mem.buffer, bufPtr, toWrite).set(chunk);
        
        currentPos += toWrite;
        total += toWrite;
      }
      
      info.pos = currentPos;
      // fdToVfs is a Map, info is a reference to the object in the map (if it was an object), 
      // but I should probably set it back just in case or if I cloned it.
      // fdToVfs.set(fd, { path: p, pos: 0, mode: 'rw' }); -> it's an object.
      // So modifying info.pos updates the object in the map.
      
      try { writeU32(nread_ptr, total); } catch (e) { }
      return 0;
    }
    return orig.fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  };
  wrapped.fd_close = function (fd) {
    // console.error("fd_close:", fd);
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd); fdToVfs.delete(fd);
      if (info.path.endsWith('.js')) {
        const cookie = path.basename(info.path, '.js'); const entry = evalVfs.get(info.path) || { js: '', json: null };
        const jsText = entry.js;
        
        const handleResult = (result) => {
             // console.error("JS Result:", result);
             const jsonPath = info.path.slice(0, -3) + '.json';
             // console.error("handleResult: jsonPath:", jsonPath);
             const jsonEntry = evalVfs.get(jsonPath) || { js: '', json: null };
             jsonEntry.json = JSON.stringify({ ok: true, result });
             // console.error("handleResult: setting entry:", jsonEntry);
             evalVfs.set(jsonPath, jsonEntry);
             if (typeof instance.exports._continue === 'function') instance.exports._continue(Number(cookie));
        };
        const handleError = (err) => {
             console.error("JS Error:", err);
             const jsonPath = info.path.slice(0, -3) + '.json';
             const jsonEntry = evalVfs.get(jsonPath) || { js: '', json: null };
             jsonEntry.json = JSON.stringify({ ok: true, result: { error: String(err) } });
             evalVfs.set(jsonPath, jsonEntry);
             if (typeof instance.exports._continue === 'function') instance.exports._continue(Number(cookie));
        };

        let isAsyncSyntax = false;
        try { new Function(jsText); } catch (e) { if (e instanceof SyntaxError) isAsyncSyntax = true; }

        if (isAsyncSyntax) {
             (async () => {
                 try {
                    const AsyncFunction = Object.getPrototypeOf(async function () { }).constructor; 
                    const fn = new AsyncFunction(jsText); 
                    handleResult(await fn());
                 } catch (err) { handleError(err); }
             })();
        } else {
            try {
                let result = eval(jsText);
                if (result instanceof Promise) {
                    result.then(handleResult).catch(handleError);
                } else {
                    handleResult(result);
                }
            } catch (err) { handleError(err); }
        }
      }
      return 0;
    }
    return orig.fd_close(fd);
  };

  wrapped.path_create_directory = function (fd, path_ptr, path_len) {
    const p = readStringFromMemory(path_ptr, path_len);
    // console.error("path_create_directory:", p);
    return orig.path_create_directory(fd, path_ptr, path_len);
  };

  importObj.wasi_snapshot_preview1 = wrapped;

  const instance = await WebAssembly.instantiate(module, importObj);
  memRef.memory = instance.exports.memory;

  // Reactor initialization
  // wasi.initialize(instance) will call _initialize export if it exists.
  try {
    if (isVerbose) console.error("Initializing WASI...");
    wasi.initialize(instance);
    if (isVerbose) console.error("WASI Initialized.");
  } catch (e) {
    if (isVerbose) console.error("WASI Init Error:", e);
    // Some Node/WASI implementations signal proc_exit by throwing an error
    // containing a special symbol like Symbol(kExitCode). Detect that case and
    // propagate the exit code to the host process so the exit builtin works.
    try {
      // Check for common numeric properties first
      if (e && typeof e === 'object') {
        if (typeof e.code === 'number') {
          process.exit(e.code);
        }
        if (typeof e.errno === 'number') {
          process.exit(e.errno);
        }
        // Inspect symbols for kExitCode
        const syms = Object.getOwnPropertySymbols(e);
        for (const s of syms) {
          if (s && s.description === 'kExitCode') {
            const code = e[s];
            if (typeof code === 'number') process.exit(code);
          }
        }
        // Fallback: parse message like 'exit code: N'
        if (e && e.message) {
          const m = String(e.message).match(/exit\s+code[:\s]+(\d+)/i);
          if (m) process.exit(parseInt(m[1], 10));
        }
      }
    } catch (ex) {
      if (isVerbose) console.error('Error while extracting WASI exit code:', ex);
    }
    // Ignore other init errors (e.g. no _initialize present)
  }

  // If stdin is not a TTY (piped input), wait for the guest to record an exit code
  // to `dev/washm_exit_code` and exit the host with that code. For interactive
  // sessions (TTY), do not automatically exit.
  if (!process.stdin.isTTY) {
    const exitPath = path.resolve(process.cwd(), 'dev', 'washm_exit_code');
    const maxWaitMs = 30000; // 30s timeout
    const start = Date.now();
    while (Date.now() - start < maxWaitMs) {
      try {
        const data = await fs.readFile(exitPath, 'utf8');
        const code = parseInt(data.trim(), 10);
        if (!Number.isNaN(code)) {
          if (isVerbose) console.error(`Guest exit code: ${code}`);
          try { await fs.unlink(exitPath); } catch (e) { }
          process.exit(code);
        }
      } catch (e) {
        // File not yet present; sleep and retry
      }
      await new Promise((r) => setTimeout(r, 50));
    }
    // Timeout: assume success
    process.exit(0);
  }

  // Start a background watcher to exit the host when the guest writes the exit file.
  (async () => {
    const exitPath = path.resolve(process.cwd(), 'dev', 'washm_exit_code');
    while (true) {
      try {
        const data = await fs.readFile(exitPath, 'utf8');
        const code = parseInt(data.trim(), 10);
        if (!Number.isNaN(code)) {
          if (isVerbose) console.error(`Guest exit code: ${code} (watcher)`);
          try { await fs.unlink(exitPath); } catch (e) { }
          process.exit(code);
        }
      } catch (e) {
        // File not yet present; continue polling
      }
      await new Promise((r) => setTimeout(r, 200));
    }
  })();
}


if (require.main === module) {
  run();
}
