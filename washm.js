#!/usr/bin/env -S node --no-warnings
// Polyglot launcher for WASHM - runs in both Node.js and Browser
(function() {
  'use strict';

  const isNode = typeof process !== 'undefined' && process.versions && process.versions.node;
  const isBrowser = typeof window !== 'undefined';

  if (isNode) {
    // Node.js setup
    const _originalEmitWarning = process.emitWarning;
    process.emitWarning = function(warning, ...args) {
      try {
        const name = (typeof args[0] === 'string' ? args[0] : (warning && warning.name)) || '';
        const message = typeof warning === 'string' ? warning : (warning && warning.message) || '';
        if ((name === 'ExperimentalWarning' || /ExperimentalWarning/.test(name)) && /WASI/.test(message)) return;
      } catch (e) { /* fall through */ }
      return _originalEmitWarning.call(process, warning, ...args);
    };

    const fs = require('fs');
    const { WASI } = require('wasi');
    const argv = process.argv;

    async function mainNode() {
      const wasmPath = __dirname + '/washm.wasm';
      const wasmBuffer = fs.readFileSync(wasmPath);

      const wasi = new WASI({
        args: ['washm.js', ...argv.slice(2)],
        env: process.env,
        preopens: (function() {
          const preopens = { '/': '/' };
          if (process.platform !== 'win32') return preopens;
          const pathModule = require('path');
          const os = require('os');
          const cwdRoot = pathModule.parse(process.cwd()).root;
          const homeRoot = pathModule.parse(os.homedir()).root;
          [cwdRoot, homeRoot].filter(Boolean).forEach(r => {
            const rootPath = r.endsWith('\\') ? r : r + '\\';
            const letter = rootPath[0].toUpperCase();
            preopens[`/${letter}:/`] = rootPath;
          });
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

    mainNode();
  } else if (isBrowser) {
    // Browser setup with minimal WASI shim
    async function mainBrowser() {
      // Initialize override hooks
      globalThis.washm_wasi_overrides = {};

      // Minimal WASI shim with delegating stubs
      const wasiSnapshot = {
        fd_write: function(fd, iovs, iovsLen, nwritten) {
          // Check for override
          if (globalThis.washm_wasi_overrides.fd_write) {
            return globalThis.washm_wasi_overrides.fd_write(fd, iovs, iovsLen, nwritten);
          }
          
          // Bootstrap FD handling
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
          
          // Default: stdout/stderr to console (buffered)
          if (fd === 1 || fd === 2) {
            const memory = new Uint8Array(instance.exports.memory.buffer);
            const view = new DataView(memory.buffer);
            let total = 0;
            let output = '';
            for (let i = 0; i < iovsLen; i++) {
              const ptr = view.getUint32(iovs + i * 8, true);
              const len = view.getUint32(iovs + i * 8 + 4, true);
              const chunk = memory.slice(ptr, ptr + len);
              output += new TextDecoder().decode(chunk);
              total += len;
            }
            if (output) console.log(output);
            view.setUint32(nwritten, total, true);
            return 0;
          }
          
          return 28; // ENOSYS
        },
        
        fd_close: function(fd) {
          // Bootstrap FD handling
          if (fd === 20251219) {
            const js = collectEvalJS;
            collectEvalJS = '';
            eval(js);
            return 0;
          }
          return 0; // Success for other fds
        },
        
        random_get: function(buf, bufLen) {
          if (globalThis.washm_wasi_overrides.random_get) {
            return globalThis.washm_wasi_overrides.random_get(buf, bufLen);
          }
          const memory = new Uint8Array(instance.exports.memory.buffer);
          for (let i = 0; i < bufLen; i++) {
            memory[buf + i] = Math.floor(Math.random() * 256);
          }
          return 0;
        },
        
        clock_time_get: function(clockId, precision, time) {
          if (globalThis.washm_wasi_overrides.clock_time_get) {
            return globalThis.washm_wasi_overrides.clock_time_get(clockId, precision, time);
          }
          const memory = instance.exports.memory;
          const view = new DataView(memory.buffer);
          const now = BigInt(Date.now()) * 1000000n; // Convert ms to ns
          view.setBigUint64(time, now, true);
          return 0;
        },
        
        fd_read: function(fd, iovs, iovsLen, nread) {
          return 28; // ENOSYS - will be overridden by TTY bridge
        },
        
        fd_seek: function() { return 28; },
        fd_filestat_get: function() { return 28; },
        fd_filestat_set_size: function() { return 28; },
        fd_filestat_set_times: function() { return 28; },
        fd_pread: function() { return 28; },
        fd_pwrite: function() { return 28; },
        fd_sync: function() { return 28; },
        fd_tell: function() { return 28; },
        fd_advise: function() { return 28; },
        fd_allocate: function() { return 28; },
        path_open: function() { return 28; },
        path_filestat_get: function() { return 28; },
        path_filestat_set_times: function() { return 28; },
        path_link: function() { return 28; },
        path_symlink: function() { return 28; },
        path_readlink: function() { return 28; },
        path_rename: function() { return 28; },
        path_remove_directory: function() { return 28; },
        path_unlink_file: function() { return 28; },
        path_create_directory: function() { return 28; },
        fd_readdir: function() { return 28; },
        fd_prestat_get: function(fd, bufPtr) {
          // FD 3 is typically the first pre-opened directory
          if (fd === 3) {
            const memory = instance.exports.memory;
            const view = new DataView(memory.buffer);
            view.setUint8(bufPtr, 0); // PREOPENTYPE_DIR
            view.setUint32(bufPtr + 4, 1, true); // path length = 1 ("/")
            return 0;
          }
          return 8; // EBADF
        },
        fd_prestat_dir_name: function(fd, pathPtr, pathLen) {
          if (fd === 3 && pathLen >= 1) {
            const memory = instance.exports.memory;
            const view = new Uint8Array(memory.buffer);
            view[pathPtr] = 47; // "/" ASCII
            return 0;
          }
          return 28;
        },
        fd_fdstat_get: function(fd, bufPtr) {
          if (fd === 3) {
            // FD 3 is the root directory
            const memory = instance.exports.memory;
            const view = new DataView(memory.buffer);
            view.setUint8(bufPtr, 3); // FILETYPE_DIRECTORY
            view.setUint16(bufPtr + 2, 0, true); // flags
            view.setBigUint64(bufPtr + 8, 0xFFFFFFFFFFFFFFFFn, true); // rights_base (all)
            view.setBigUint64(bufPtr + 16, 0xFFFFFFFFFFFFFFFFn, true); // rights_inheriting (all)
            return 0;
          }
          return 28;
        },
        fd_fdstat_set_flags: function() { return 28; },
        fd_fdstat_set_rights: function() { return 28; },
        sock_accept: function() { return 28; },
        sock_recv: function() { return 28; },
        sock_send: function() { return 28; },
        sock_shutdown: function() { return 28; },
        environ_sizes_get: function(environc, environBufSize) {
          const memory = instance.exports.memory;
          const view = new DataView(memory.buffer);
          view.setUint32(environc, 0, true);
          view.setUint32(environBufSize, 0, true);
          return 0;
        },
        environ_get: function() { return 0; },
        args_sizes_get: function(argc, argvBufSize) {
          const memory = instance.exports.memory;
          const view = new DataView(memory.buffer);
          view.setUint32(argc, 1, true);
          const arg = 'washm.wasm';
          view.setUint32(argvBufSize, arg.length + 1, true);
          return 0;
        },
        args_get: function(argv, argvBuf) {
          const memory = instance.exports.memory;
          const view = new DataView(memory.buffer);
          const encoder = new TextEncoder();
          const arg = 'washm.wasm';
          const encoded = encoder.encode(arg + '\0');
          new Uint8Array(memory.buffer).set(encoded, argvBuf);
          view.setUint32(argv, argvBuf, true);
          return 0;
        },
        proc_exit: function(code) {
          console.log('Process exit with code:', code);
          throw new Error('WASI proc_exit: ' + code);
        },
        sched_yield: function() { return 0; },
        poll_oneoff: function() { return 28; },
      };

      // Wrap each function to allow runtime override
      const wrappedSnapshot = {};
      for (const key in wasiSnapshot) {
        wrappedSnapshot[key] = function() {
          return wrappedSnapshot[key].actual.apply(this, arguments);
        };
        wrappedSnapshot[key].actual = wasiSnapshot[key];
      }

      let collectEvalJS = "";
      let instance;
      const importObj = {
        wasi_snapshot_preview1: wrappedSnapshot
      };

      // Fetch and instantiate WASM
      const response = await fetch('./washm.wasm');
      const result = await WebAssembly.instantiateStreaming(response, importObj);
      instance = result.instance;

      globalThis.__washm_bootstrap = {
        importObj,
        wasiSnapshot: wrappedSnapshot,
        instance,
        require: null, // Not available in browser
      };

      // Set platform before starting
      if (instance.exports.washm_set_platform) {
        instance.exports.washm_set_platform(1); // 1 = browser
      }

      // Start the WASM module
      if (instance.exports._start) {
        instance.exports._start();
      } else if (instance.exports._initialize) {
        instance.exports._initialize();
      }
    }

    mainBrowser().catch(err => {
      console.error('Failed to start WASHM:', err);
      document.body.innerHTML = '<pre style="color: red; padding: 20px;">Error: ' + err.message + '</pre>';
    });
  }
})();
