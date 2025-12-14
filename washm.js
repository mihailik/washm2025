#!/usr/bin/env node
// @ts-check

/**
 * Node host for WASI Async interop. Implements the /dev/host/eval protocol + _continue.
   This host uses `node:wasi` and expects a WASM guest compiled for `wasm32-wasi`.
*/
async function run() {

  const fs = require('fs/promises');
  const path = require('path');
  const { TextDecoder, TextEncoder } = require('util');
  const { execSync } = require('child_process');
  const os = require('os');

  const cliArgs = process.argv.slice(2);
  const isVerbose = cliArgs.includes("--verbose");

  let _origEmitWarning = null;
  let _didOverride = false;
  if (!isVerbose && typeof process !== 'undefined' && typeof process.emitWarning === 'function') {
    _origEmitWarning = process.emitWarning;
    process.emitWarning = function (warning, ...rest) {
      try {
        const name = warning && typeof warning === 'object' && warning.name ? warning.name : undefined;
        const msg = typeof warning === 'string' ? warning : (warning && warning.message) || '';
        if ((name && name === 'ExperimentalWarning') || (msg && /WASI is an experimental feature/i.test(msg))) {
          return;
        }
      } catch (e) { }
      return _origEmitWarning.call(process, warning, ...rest);
    };
    _didOverride = true;
  }

  let wasmPath = cliArgs.find(a => !a.startsWith('-'));
  if (wasmPath) {
    try { await fs.access(wasmPath); } catch (e) { wasmPath = null; }
  }
  
  if (!wasmPath) {
    const viablePaths = ['.', 'target/wasm32-wasip1/release', 'target/wasm32-wasip1/debug'];
    for (let i = 0; i < viablePaths.length; i++) {
      const p = viablePaths[i];
      const candidate = path.resolve(__dirname, p, 'washm.wasm');
      if (i === viablePaths.length - 1) {
        try { await fs.access(candidate); wasmPath = candidate; } catch(e) {}
      } else {
        try { await fs.access(candidate); wasmPath = candidate; break; } catch (e) { }
      }
    }
  }

  let guestArgs = [];
  if (wasmPath) {
    const wasmIndex = cliArgs.indexOf(wasmPath);
    if (wasmIndex >= 0) {
      guestArgs = [path.basename(wasmPath), ...cliArgs.slice(wasmIndex + 1)];
    } else {
      guestArgs = [path.basename(wasmPath), ...cliArgs];
    }
  } else {
    guestArgs = [];
  }
  const wasiArgs = guestArgs;
  let WASI = require('wasi').WASI;
  if (_didOverride && _origEmitWarning) {
    process.emitWarning = _origEmitWarning;
  }

  // Initialize globalThis.mounted
  const evalState = new Map(); // Stores { js: string, json: string } for each cookie
  
  globalThis.mounted = {
    washm: {
        get start_pwd() { return process.cwd(); },        // JSON string exposing startup parameters: env, user, cwd, args
        startup: JSON.stringify({
            env: Object.assign({}, process.env),
            user: process.env.USER || os.userInfo().username,
            cwd: process.cwd(),
            args: wasiArgs
        }),        set exit_code(val) { 
            if (isVerbose) console.error(`Guest requested exit with code: ${val}`);
            process.exit(Number(val)); 
        }
    },
    eval: new Proxy({}, {
        get(target, prop) {
            if (typeof prop === 'string') {
                const parts = prop.split('.');
                const cookie = parts[0];
                const ext = parts[1];
                
                if (!evalState.has(cookie)) {
                    evalState.set(cookie, { js: '', json: null });
                }
                const state = evalState.get(cookie);

                if (ext === 'js') {
                    return {
                        _isVirtualFile: true,
                        write: (chunk) => { state.js += chunk; },
                        close: () => {
                            // Execute JS
                            const jsText = state.js;
                            const handleResult = (result) => {
                                state.json = JSON.stringify({ ok: true, result });
                                if (typeof instance.exports._continue === 'function') instance.exports._continue(Number(cookie));
                            };
                            const handleError = (err) => {
                                console.error("JS Error:", err);
                                state.json = JSON.stringify({ ok: true, result: { error: String(err) } });
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
                    };
                } else if (ext === 'json') {
                    return {
                        _isVirtualFile: true,
                        read: () => state.json || ''
                    };
                }
            }
            return Reflect.get(target, prop);
        }
    })
  };

  const wasi = new WASI({
    args: wasiArgs,
    env: Object.assign({}, process.env),
    // Preopen / to / to allow full filesystem access and correct path mapping
    preopens: (function() {
        const p = {};
        if (process.platform === 'win32') {
            // On Windows, map drives like C:\ to /C:/
            let found = false;
            
            // Method 1: wmic (Standard, no admin required, works on most versions)
            // Output: "Name\r\nC:\r\nD:"
            try {
                const stdout = execSync('wmic logicaldisk get name', { encoding: 'utf8', stdio: 'pipe', windowsHide: true });
                const lines = stdout.split(/[\r\n]+/);
                const drives = lines
                    .map(l => l.trim())
                    .filter(l => /^[A-Z]:$/.test(l)); // Matches "C:"
                
                if (drives.length > 0) {
                    for (const drive of drives) {
                        // drive is "C:", we need "C:\" for value
                        p[`/${drive}/`] = drive + '\\';
                    }
                    found = true;
                }
            } catch (e) { }

            // Method 2: fsutil (Fast, but requires Admin)
            if (!found) {
                try {
                    const stdout = execSync('fsutil fsinfo drives', { encoding: 'utf8', stdio: 'pipe', windowsHide: true });
                    const drives = stdout.match(/[A-Z]:\\/g);
                    if (drives) {
                        for (const drive of drives) {
                            const letter = drive.substring(0, 2); // "C:"
                            p[`/${letter}/`] = drive;
                        }
                        found = true;
                    }
                } catch (e) { }
            }

            if (!found) {
                // Fallback: map the drive of the current working directory
                const cwdRoot = path.parse(process.cwd()).root; // e.g. "D:\"
                if (cwdRoot.endsWith(':\\')) {
                    const drive = cwdRoot.substring(0, 2); // "D:"
                    p[`/${drive}/`] = cwdRoot;
                }
                
                // Always try C: if it's not the current drive
                if (cwdRoot.toUpperCase() !== 'C:\\') {
                     p['/C:/'] = 'C:\\';
                }
            }
        } else {
            p['/'] = '/';
        }
        return p;
    })(),
    version: 'preview1'
  });

  if (isVerbose) console.error("Loading WASM...");
  const wasm = await fs.readFile(wasmPath);
  const module = await WebAssembly.compile(wasm);
  const importObj = { wasi_snapshot_preview1: wasi.wasiImport };

  const fdToVfs = new Map();
  let nextFakeFd = 2000; // Start high to avoid conflict
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

  function resolveMounted(p) {
      // p is like "dev/host/washm/start_pwd"
      const parts = p.split('/');
      if (parts[0] === 'dev' && parts[1] === 'host') {
          let curr = globalThis.mounted;
          for (let i = 2; i < parts.length; i++) {
              if (!curr) return undefined;
              curr = curr[parts[i]];
          }
          return curr;
      }
      return undefined;
  }

  wrapped.path_open = function (dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr) {
    const p = readStringFromMemory(path_ptr, path_len);
    // Check if path targets /dev/host
    // Since we preopen / as root, paths might come in as "dev/host/..."
    if (p.startsWith('dev/host/') || p === 'dev/host') {
        const target = resolveMounted(p);
        if (target) {
            const fd = nextFakeFd++;
            // If it's a virtual file object (from proxy)
            if (target._isVirtualFile) {
                fdToVfs.set(fd, { target, pos: 0, mode: 'rw' });
            } else if (typeof target === 'string') {
                // Read-only string property
                fdToVfs.set(fd, { content: target, pos: 0, mode: 'r' });
            } else {
                // It's a directory or object, treat as directory?
                fdToVfs.set(fd, { isDir: true });
            }
            writeU32(fd_out_ptr, fd);
            return 0;
        } else {
            // Special handling for exit_code
            if (p === 'dev/host/washm/exit_code') {
                const fd = nextFakeFd++;
                fdToVfs.set(fd, { 
                    write: function(chunk) { 
                        if (!this.buf) this.buf = '';
                        this.buf += chunk;
                    },
                    close: function() {
                        globalThis.mounted.washm.exit_code = this.buf;
                    }
                });
                writeU32(fd_out_ptr, fd);
                return 0;
            }
        }
    }
    return orig.path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };

  wrapped.fd_write = function (fd, iovs_ptr, iovs_len, nwritten_ptr) {
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd);
      const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      for (let i = 0; i < iovs_len; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const bytes = new Uint8Array(mem.buffer, bufPtr, bufLen);
        const text = new TextDecoder('utf-8').decode(bytes);
        
        if (info.target && info.target.write) {
            info.target.write(text);
        } else if (info.write) {
            info.write.call(info, text);
        }
        total += bufLen;
      }
      try { writeU32(nwritten_ptr, total); } catch (e) { }
      return 0;
    }
    return orig.fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr);
  };

  wrapped.fd_read = function (fd, iovs_ptr, iovs_len, nread_ptr) {
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd);
      let content = '';
      if (info.content) {
          content = info.content;
      } else if (info.target && info.target.read) {
          content = info.target.read();
      }

      const bytes = new TextEncoder().encode(content);
      const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      let currentPos = info.pos || 0;

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
      try { writeU32(nread_ptr, total); } catch (e) { }
      return 0;
    }
    return orig.fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  };

  wrapped.fd_close = function (fd) {
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd);
      fdToVfs.delete(fd);
      if (info.target && info.target.close) {
          info.target.close();
      } else if (info.close) {
          info.close.call(info);
      }
      return 0;
    }
    return orig.fd_close(fd);
  };

  wrapped.path_create_directory = function (fd, path_ptr, path_len) {
    const p = readStringFromMemory(path_ptr, path_len);
    if (p.startsWith('dev/host/') || p === 'dev/host') {
        return 0;
    }
    return orig.path_create_directory(fd, path_ptr, path_len);
  };

  wrapped.path_filestat_get = function (fd, flags, path_ptr, path_len, buf_ptr) {
      const p = readStringFromMemory(path_ptr, path_len);
      if (p.startsWith('dev/host/') || p === 'dev/host') {
          const target = resolveMounted(p);
          if (target) {
              const isDir = !target._isVirtualFile && typeof target !== 'string';
              const type = isDir ? 3 : 4;
              const mem = memRef.memory;
              const dv = new DataView(mem.buffer);
              dv.setBigUint64(buf_ptr, 0n, true);
              dv.setBigUint64(buf_ptr + 8, 0n, true);
              dv.setUint8(buf_ptr + 16, type);
              dv.setBigUint64(buf_ptr + 24, 1n, true);
              dv.setBigUint64(buf_ptr + 32, 0n, true);
              dv.setBigUint64(buf_ptr + 40, 0n, true);
              dv.setBigUint64(buf_ptr + 48, 0n, true);
              dv.setBigUint64(buf_ptr + 56, 0n, true);
              return 0;
          }
          return 44; // ENOENT
      }
      return orig.path_filestat_get(fd, flags, path_ptr, path_len, buf_ptr);
  };

  wrapped.path_unlink_file = function (fd, path_ptr, path_len) {
    const p = readStringFromMemory(path_ptr, path_len);
    if (p.startsWith('dev/host/') || p === 'dev/host') {
        if (p.startsWith('dev/host/eval/')) {
            const parts = p.split('/');
            const filename = parts[parts.length - 1];
            const cookie = filename.split('.')[0];
            if (evalState.has(cookie)) {
                evalState.delete(cookie);
            }
        }
        return 0;
    }
    return orig.path_unlink_file(fd, path_ptr, path_len);
  };

  importObj.wasi_snapshot_preview1 = wrapped;

  const instance = await WebAssembly.instantiate(module, importObj);
  memRef.memory = instance.exports.memory;

  try {
    wasi.initialize(instance);
  } catch (e) {
    if (e && typeof e === 'object') {
        if (typeof e.code === 'number') process.exit(e.code);
        if (e.message) {
          const m = String(e.message).match(/exit\s+code[:\s]+(\d+)/i);
          if (m) process.exit(parseInt(m[1], 10));
        }
    }
  }
}

run().catch(err => {
  console.error(err);
  throw err;
});
