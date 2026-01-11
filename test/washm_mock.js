// @ts-check
/**
 * WASHM Test Harness - Mock Environment for Testing
 * 
 * This module provides a test harness that executes the real washm.js
 * in mocked environments (Node.js or browser) to test both code paths.
 * 
 * The harness provides minimal mocked APIs and evaluates washm.js in a
 * controlled scope, enabling comprehensive testing without modifying
 * production code.
 */

// ============================================================================
// Shared Mock Components
// ============================================================================

/**
 * Mock stdin buffer for test control
 */
class MockStdin {
  constructor() {
    this.buffer = [];
    this.closed = false;
  }

  write(str) {
    if (this.closed) {
      throw new Error('Cannot write to closed stdin');
    }
    this.buffer.push(str);
  }

  read() {
    if (this.buffer.length === 0) {
      return null;
    }
    return this.buffer.shift();
  }

  end() {
    this.closed = true;
  }

  // Stream-like methods for compatibility
  setEncoding(encoding) {
    // Stub - encoding is ignored
  }

  on(event, handler) {
    // Stub - event listener
  }

  once(event, handler) {
    // Stub - one-time event listener
  }

  removeListener(event, handler) {
    // Stub - remove event listener
  }

  pause() {
    // Stub
  }

  resume() {
    // Stub
  }
}

/**
 * Mock stdout/stderr buffer for test assertions
 */
class MockStdout {
  constructor() {
    this.chunks = [];
  }

  write(chunk) {
    if (typeof chunk === 'string') {
      this.chunks.push(chunk);
    } else if (chunk instanceof Uint8Array || Array.isArray(chunk)) {
      try {
        this.chunks.push(new TextDecoder().decode(new Uint8Array(chunk)));
      } catch (e) {
        this.chunks.push(String(chunk));
      }
    } else {
      this.chunks.push(String(chunk));
    }
  }

  readAll() {
    return this.chunks.join('');
  }

  clear() {
    this.chunks = [];
  }
}
/**
 * Mock Terminal for Browser Mode (xterm.js compatible)
 */
class MockTerminal {
  constructor(streams) {
    this.streams = streams;
    this.rows = 24;
    this.cols = 80;
    this._onDataListeners = [];
    this._onResizeListeners = [];
  }

  // xterm.js API: write to display
  write(data) {
    // In browser mode, terminal output goes to display.
    // We capture this into our stdout stream for testing assertions.
    this.streams.stdout.write(data);
  }

  // xterm.js API: handle input from user
  onData(fn) {
    this._onDataListeners.push(fn);
    return { dispose: () => { } };
  }

  onResize(fn) {
    this._onResizeListeners.push(fn);
    return { dispose: () => { } };
  }

  // Other xterm.js APIs
  loadAddon() { }
  open() { }
  focus() { }

  // Test Helper: Simulate user typing
  input(data) {
    this._onDataListeners.forEach(fn => fn(data));
  }
}

/**
 * Create mock process object
 */
function createMockProcess(opts, streams) {
  let mockCwd = opts.cwd || '/';

  return {
    cwd: () => mockCwd,
    chdir: (dir) => { mockCwd = dir; },
    platform: opts.platform || 'linux',
    arch: opts.arch || 'x64',
    env: { ...opts.env },
    argv: ['node', 'washm.js'],
    version: 'v20.0.0',
    versions: { node: '20.0.0' },
    stdin: streams.stdin,
    stdout: streams.stdout,
    stderr: streams.stderr,
    exit: (code) => { throw new Error(`process.exit(${code})`); },
    emitWarning: () => { },
  };
}

/**
 * Create mock os module
 */
function createMockOsModule(opts) {
  return {
    homedir: () => opts.homedir || '/home/user',
    tmpdir: () => opts.tmpdir || '/tmp',
    platform: () => opts.platform || 'linux',
    arch: () => opts.arch || 'x64',
    cpus: () => [{ model: 'mock-cpu', speed: 2400 }],
    hostname: () => opts.hostname || 'localhost',
    totalmem: () => 8589934592, // 8GB
    freemem: () => 4294967296, // 4GB
    type: () => 'Linux',
    release: () => '5.0.0',
    userInfo: (options) => ({
      username: 'user',
      uid: 1000,
      gid: 1000,
      shell: '/bin/bash',
      homedir: opts.homedir || '/home/user',
    }),
    uptime: () => 86400, // 1 day
    loadavg: () => [0.5, 0.3, 0.2],
    networkInterfaces: () => ({}),
    EOL: '\n',
  };
}

/**
 * Create mock fs module
 */
function createMockFsModule() {
  const files = new Map();

  return {
    readFileSync: (path) => {
      const content = files.get(path);
      if (content === undefined) {
        const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
        error.code = 'ENOENT';
        throw error;
      }
      return content;
    },
    writeFileSync: (path, data) => {
      files.set(path, data);
    },
    existsSync: (path) => files.has(path),
    _files: files, // For test inspection
  };
}

/**
 * Create mock tty module
 */
function createMockTtyModule() {
  return {
    isatty: (fd) => false,
  };
}

/**
 * Create mock child_process module
 */
function createMockChildProcessModule() {
  return {
    spawn: () => {
      throw new Error('child_process.spawn is not supported in test environment');
    },
  };
}

/**
 * Create mock require function
 */
function createMockRequire(mockFs, mockOs, mockTty, mockChildProcess, mockWasi) {
  return (moduleName) => {
    switch (moduleName) {
      case 'fs': return mockFs;
      case 'os': return mockOs;
      case 'tty': return mockTty;
      case 'child_process': return mockChildProcess;
      case 'wasi': return mockWasi || { WASI: class WASI { } };
      case 'path':
        // Return a minimal mock path module
        return {
          parse: (p) => ({ root: p[0] === '/' ? '/' : '' }),
          join: (...parts) => parts.join('/'),
          dirname: (p) => p.split('/').slice(0, -1).join('/') || '/',
          basename: (p) => p.split('/').pop(),
        };
      default:
        throw new Error(`Module '${moduleName}' is not mocked`);
    }
  };
}

// ============================================================================
// WashmInstance Wrapper
// ============================================================================

/**
 * Unified wrapper for both Node and Browser mode instances
 */
class WashmInstance {
  constructor(runtime, streams) {
    this.streams = streams;
    this._runtime = runtime;
  }

  get exitCode() {
    return this._runtime.exitCode;
  }

  async waitForExit() {
    if (this._runtime.waitForExit) {
      return await this._runtime.waitForExit();
    }
    return this.exitCode;
  }
}

// ============================================================================
// Node Preloader
// ============================================================================

/**
 * Minimal MockWASI for Phase 3 - handles FD 1/2 and proc_exit
 */
class MockWASI {
  constructor(streams, args = []) {
    this.streams = streams;
    this.exitCode = null;
    this.instance = null; // Will be set when WASM instantiates
    this.args = ['washm', ...args]; // Program name + arguments
  }

  /**
   * Write to file descriptor
   */
  fd_write(fd, iovs, iovsLen, nwritten) {
    if (!this.instance) {
      console.warn('fd_write called before WASM instance is set');
      return 28; // ENOSYS
    }

    const memory = new Uint8Array(this.instance.exports.memory.buffer);
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

    // Route to appropriate stream
    if (fd === 1) {
      this.streams.stdout.write(output);
    } else if (fd === 2) {
      this.streams.stderr.write(output);
    }

    view.setUint32(nwritten, total, true);
    return 0; // Success
  }

  /**
   * Close file descriptor - minimal stub
   */
  fd_close(fd) {
    return 0; // Success
  }

  /**
   * Process exit
   */
  proc_exit(code) {
    this.exitCode = code;
    throw new Error(`proc_exit(${code})`);
  }

  /**
   * Get random bytes - minimal implementation
   */
  random_get(buf, bufLen) {
    if (!this.instance) return 28;
    const memory = new Uint8Array(this.instance.exports.memory.buffer);
    for (let i = 0; i < bufLen; i++) {
      memory[buf + i] = Math.floor(Math.random() * 256);
    }
    return 0;
  }

  /**
   * Get time - minimal implementation
   */
  clock_time_get(clockId, precision, time) {
    if (!this.instance) return 28;
    const memory = new Uint8Array(this.instance.exports.memory.buffer);
    const view = new DataView(memory.buffer);
    const now = BigInt(Date.now()) * 1000000n; // nanoseconds
    view.setBigUint64(time, now, true);
    return 0;
  }

  /**
   * Environment variable count - stub
   */
  environ_sizes_get(environc, environBufSize) {
    if (!this.instance) return 28;
    const memory = new Uint8Array(this.instance.exports.memory.buffer);
    const view = new DataView(memory.buffer);
    view.setUint32(environc, 0, true);
    view.setUint32(environBufSize, 0, true);
    return 0;
  }

  /**
   * Get environment variables - stub
   */
  environ_get(environ, environBuf) {
    return 0;
  }

  /**
   * Get command line argument sizes
   */
  args_sizes_get(argc, argvBufSize) {
    if (!this.instance) return 28;
    const view = new DataView(this.instance.exports.memory.buffer);

    const argCount = this.args.length;
    const bufSize = this.args.reduce((sum, arg) => sum + arg.length + 1, 0);

    view.setUint32(argc, argCount, true);
    view.setUint32(argvBufSize, bufSize, true);
    return 0;
  }

  /**
   * Get command line arguments
   */
  args_get(argv, argvBuf) {
    if (!this.instance) return 28;
    const memory = new Uint8Array(this.instance.exports.memory.buffer);
    const view = new DataView(memory.buffer);

    let bufOffset = argvBuf;
    for (let i = 0; i < this.args.length; i++) {
      // Write pointer to argv array
      view.setUint32(argv + i * 4, bufOffset, true);

      // Write string to buffer
      const arg = this.args[i];
      for (let j = 0; j < arg.length; j++) {
        memory[bufOffset++] = arg.charCodeAt(j);
      }
      memory[bufOffset++] = 0; // Null terminator
    }

    return 0;
  }

  /**
   * Read from file descriptor - stub (not implemented in Phase 3)
   */
  fd_read(fd, iovs, iovsLen, nread) {
    return 28; // ENOSYS - not implemented yet
  }

  /**
   * Get preopen directories - stub
   */
  fd_prestat_get(fd, prestat) {
    return 8; // EBADF
  }

  /**
   * Get preopen directory name - stub
   */
  fd_prestat_dir_name(fd, path, pathLen) {
    return 8; // EBADF
  }

  /**
   * Read directory - stub
   */
  fd_readdir(fd, buf, bufLen, cookie, bufused) {
    if (!this.instance) return 28;
    const memory = new Uint8Array(this.instance.exports.memory.buffer);
    const view = new DataView(memory.buffer);
    view.setUint32(bufused, 0, true);
    return 0;
  }

  /**
   * Seek file - stub
   */
  fd_seek(fd, offset, whence, newOffset) {
    return 28; // ENOSYS
  }

  /**
   * Get file stats - stub
   */
  fd_filestat_get(fd, buf) {
    return 28; // ENOSYS
  }

  /**
   * Get file/path stats - stub
   */
  path_filestat_get(fd, flags, path, pathLen, buf) {
    return 28; // ENOSYS
  }

  /**
   * Open file/directory - stub
   */
  path_open(dirfd, dirflags, path, pathLen, oflags, fsRightsBase, fsRightsInheriting, fdflags, fd) {
    return 28; // ENOSYS
  }

  /**
   * Rename path - stub
   */
  path_rename(oldFd, oldPath, oldPathLen, newFd, newPath, newPathLen) {
    return 28; // ENOSYS
  }

  /**
   * Remove directory - stub
   */
  path_remove_directory(fd, path, pathLen) {
    return 28; // ENOSYS
  }

  /**
   * Unlink file - stub
   */
  path_unlink_file(fd, path, pathLen) {
    return 28; // ENOSYS
  }

  /**
   * Create directory - stub
   */
  path_create_directory(fd, path, pathLen) {
    return 28; // ENOSYS
  }

  /**
   * Poll oneoff - stub
   */
  poll_oneoff(in_ptr, out_ptr, nsubscriptions, nevents) {
    return 28; // ENOSYS
  }

  /**
   * Set FD flags - stub
   */
  fd_fdstat_set_flags(fd, flags) {
    return 0;
  }

  /**
   * Advise - stub
   */
  fd_advise(fd, offset, len, advice) {
    return 0;
  }

  /**
   * Yield execution - stub
   */
  sched_yield() {
    return 0;
  }

  /**
   * Get FD stats - stub
   */
  fd_fdstat_get(fd, stat) {
    return 28; // ENOSYS
  }
}

/**
 * Node preloader - creates minimal Node.js-like environment with MockWASI
 */
async function nodePreloader(opts, streams) {
  const fsModule = await import('fs');
  const pathModule = await import('path');
  const urlModule = await import('url');

  // Read washm.js file
  const __dirname_real = pathModule.dirname(urlModule.fileURLToPath(import.meta.url));
  const washmJsPath = pathModule.join(__dirname_real, '..', 'washm.js');
  let washmJs = fsModule.readFileSync(washmJsPath, 'utf-8');

  // Strip shebang if present
  if (washmJs.startsWith('#!')) {
    washmJs = washmJs.substring(washmJs.indexOf('\n') + 1);
  }

  // Read WASM file
  const wasmPath = pathModule.join(__dirname_real, '..', 'washm.wasm');
  const wasmBuffer = fsModule.readFileSync(wasmPath);

  // Create mock WASI with command line arguments
  const mockWasi = new MockWASI(streams, opts.args || []);

  // Create WASI import object with delegating wrappers
  const wasiSnapshot = {};
  const wasiMethods = [
    'fd_write', 'fd_close', 'proc_exit', 'random_get', 'clock_time_get',
    'environ_sizes_get', 'environ_get', 'args_sizes_get', 'args_get',
    'fd_read', 'fd_prestat_get', 'fd_prestat_dir_name', 'fd_readdir',
    'fd_seek', 'fd_filestat_get', 'path_filestat_get', 'path_open',
    'path_rename', 'path_remove_directory', 'path_unlink_file', 'path_create_directory',
    'poll_oneoff', 'fd_fdstat_set_flags', 'fd_advise', 'sched_yield', 'fd_fdstat_get'
  ];

  wasiMethods.forEach(method => {
    wasiSnapshot[method] = function () {
      return wasiSnapshot[method].actual.apply(this, arguments);
    };
    if (mockWasi[method]) {
      wasiSnapshot[method].actual = mockWasi[method].bind(mockWasi);
    } else {
      // Create a stub for any missing methods
      wasiSnapshot[method].actual = function () {
        console.warn(`WASI stub called: ${method}`);
        return 28; // ENOSYS - not implemented
      };
    }
  });

  // Add any additional WASI methods that might be needed
  const additionalMethods = [
    'fd_tell', 'fd_sync', 'fd_allocate', 'fd_pread', 'fd_pwrite',
    'path_symlink', 'path_link', 'path_readlink', 'sock_recv', 'sock_send'
  ];

  additionalMethods.forEach(method => {
    if (!wasiSnapshot[method]) {
      wasiSnapshot[method] = function () {
        return wasiSnapshot[method].actual.apply(this, arguments);
      };
      wasiSnapshot[method].actual = function () {
        return 28; // ENOSYS
      };
    }
  });

  // Create mock WASI class
  class MockWASIClass {
    constructor(options) {
      this.wasiImport = wasiSnapshot;
    }

    start(instance) {
      // Set the instance reference for MockWASI
      mockWasi.instance = instance;

      // WASM start - execute the _start function
      if (instance.exports._start) {
        try {
          instance.exports._start();
        } catch (err) {
          if (!err.message || !err.message.startsWith('proc_exit(')) {
            throw err;
          }
        }
      }
    }
  }

  // Create mock fs module that can read WASM file
  const mockFs = createMockFsModule();

  // Normalize paths for cross-platform compatibility
  const normalizeFilePath = (p) => {
    if (typeof p !== 'string') return p;
    return p.replace(/\\/g, '/').replace(/\/+/g, '/');
  };

  const normalizedWasmPath = normalizeFilePath(wasmPath);
  mockFs._files.set(normalizedWasmPath, wasmBuffer);

  mockFs.readFileSync = (filepath, options) => {
    const normalizedPath = normalizeFilePath(filepath);
    // Allow reading the actual WASM file
    if (normalizedPath === normalizedWasmPath) {
      return wasmBuffer;
    }
    const content = mockFs._files.get(normalizedPath);
    if (content === undefined) {
      const error = new Error(`ENOENT: no such file or directory, open '${filepath}'`);
      error.code = 'ENOENT';
      throw error;
    }
    return content;
  };

  // ============================================================================
  // Execute via direct eval with mocked scope
  // ============================================================================

  // Declare variables expected by washm.js in this scope
  const process = createMockProcess(opts, streams);
  const require = createMockRequire(
    mockFs,
    createMockOsModule(opts),
    createMockTtyModule(),
    createMockChildProcessModule(),
    { WASI: MockWASIClass }
  );
  // __dirname is used in washm.js for finding washm.wasm
  const __dirname = pathModule.dirname(wasmPath);

  // mounted is not used in node path of washm.js but might be used by protocol.js if we were testing that
  const mounted = {};

  // Evaluate washm.js in this scope
  // Variables above (process, require, __dirname) will be used by the script
  try {
    eval(washmJs);
  } catch (err) {
    if (err.message && err.message.startsWith('proc_exit(')) {
      // Expected exit - extract code
      const match = err.message.match(/proc_exit\((\d+)\)/);
      if (match) {
        mockWasi.exitCode = parseInt(match[1], 10);
      }
    } else {
      throw err;
    }
  }

  return {
    streams,
    get exitCode() {
      return mockWasi.exitCode !== null ? mockWasi.exitCode : 0;
    },
    waitForExit: async () => {
      // Already exited during eval
      return mockWasi.exitCode !== null ? mockWasi.exitCode : 0;
    },
  };
}

// ============================================================================
// Browser Preloader
// ============================================================================

const realGlobal = globalThis;

/**
 * Browser preloader - creates minimal Browser-like environment
 * Uses direct eval() with local mocked variables to effectively shadow globals.
 */
async function browserPreloader(opts, streams) {
  const fsModule = await import('fs');
  const pathModule = await import('path');
  const urlModule = await import('url');

  // Read files
  const __dirname_real = pathModule.dirname(urlModule.fileURLToPath(import.meta.url));
  const washmJsPath = pathModule.join(__dirname_real, '..', 'washm.js');
  let washmJs = fsModule.readFileSync(washmJsPath, 'utf-8');

  if (washmJs.startsWith('#!')) {
    washmJs = washmJs.substring(washmJs.indexOf('\n') + 1);
  }

  const wasmPath = pathModule.join(__dirname_real, '..', 'washm.wasm');
  const wasmBuffer = fsModule.readFileSync(wasmPath);

  // Initialize components
  const terminal = new MockTerminal(streams);

  // Input Queue for washm_tty
  const inputQueue = [];
  let inputNotify = null;

  function processQueue() {
    if (inputNotify && inputQueue.length > 0) {
      const resolve = inputNotify;
      inputNotify = null;
      resolve(inputQueue.shift());
    }
  }

  // Link stdin write to terminal input
  const originalStdinWrite = streams.stdin.write;
  streams.stdin.write = (chunk) => {
    originalStdinWrite.call(streams.stdin, chunk);
    terminal.input(chunk);

    // Push to queue for washm_tty.read()
    if (typeof chunk === 'string') {
      for (const char of chunk) {
        inputQueue.push(char);
      }
    } else {
      inputQueue.push(chunk);
    }
    processQueue();
  };

  // Exit code tracking
  let exitCode = 0;

  // ============================================================================
  // DECLARE MOCKED GLOBALS IN SCOPE
  // ============================================================================

  // CRITICAL: Shadow 'process' and 'require' to force isNode = false
  var process;
  var require;

  // Window & Document Mocks
  const window = {
    addEventListener: () => { },
    removeEventListener: () => { },
    location: { hostname: 'localhost' },
    sessionStorage: {
      _store: new Map(),
      getItem(k) { return this._store.get(k) || null; },
      setItem(k, v) { this._store.set(k, v); },
      removeItem(k) { this._store.delete(k); },
      clear() { this._store.clear(); }
    },
    // Expose Terminal on window for xterm-addon-fit or others that might look for it
    Terminal: function () { return terminal; },
    FitAddon: class { fit() { } }
  };
  // Add FitAddon statically
  window.FitAddon.FitAddon = window.FitAddon;

  const document = {
    addEventListener: () => { },
    removeEventListener: () => { },
    createElement: (tag) => {
      // Return dummy element
      return new Proxy({
        style: {},
        appendChild: () => { },
        setAttribute: () => { },
        textContent: '',
        src: '',
        addEventListener: () => { }
      }, {
        get: (target, prop) => target[prop] || (() => { })
      });
    },
    body: {
      style: {},
      appendChild: () => { },
      innerHTML: '',
      removeChild: () => { }
    },
    head: {
      appendChild: () => { },
      removeChild: () => { }
    },
    title: ''
  };

  const navigator = {
    userAgent: 'Mozilla/5.0 (Test Environment) MockBrowser/1.0',
    hardwareConcurrency: opts.cpus || 4,
    deviceMemory: 8,
    platform: 'Linux x86_64'
  };

  // Mock Fetch
  const fetch = async (url) => {
    // Determine path relative to script
    const normalizedUrl = url.toString().replace(/\\/g, '/');
    if (normalizedUrl.endsWith('washm.wasm')) {
      return {
        ok: true,
        arrayBuffer: async () => wasmBuffer
      };
    }
    throw new Error(`Fetch 404: ${url}`);
  };

  // Mock WebAssembly with instantiateStreaming polyfill
  const WebAssembly = {
    ...realGlobal.WebAssembly,
    instantiateStreaming: async (responsePromise, importObject) => {
      const response = await responsePromise;
      const buffer = await response.arrayBuffer();

      // Inject Argument Overrides into WASI snapshot if available
      if (importObject && importObject.wasi_snapshot_preview1) {
        const wasi = importObject.wasi_snapshot_preview1;
        const args = opts.args || ['washm'];
        // Ensure program name is first
        if (args.length === 0 || args[0] !== 'washm') args.unshift('washm');

        // Override args_sizes_get
        const originalArgsSizesGet = wasi.args_sizes_get.actual || wasi.args_sizes_get;
        wasi.args_sizes_get.actual = function (argc, argvBufSize) {
          const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
          // If 'this' has exports (upstream WASI behavior), use it. Otherwise use global instance.
          const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);

          if (!memory) {
            throw new Error('WASI args_sizes_get: Memory not found');
          }

          const view = new DataView(memory.buffer);
          view.setUint32(argc, args.length, true);

          let totalSize = 0;
          for (const arg of args) {
            totalSize += new TextEncoder().encode(arg).length + 1; // +1 for null terminator
          }
          view.setUint32(argvBufSize, totalSize, true);
          return 0;
        };

        // Override args_get
        const originalArgsGet = wasi.args_get.actual || wasi.args_get;
        wasi.args_get.actual = function (argv, argvBuf) {
          const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
          const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);

          if (!memory) throw new Error('WASI args_get: Memory not found');

          const view = new DataView(memory.buffer);
          const memoryBytes = new Uint8Array(memory.buffer);
          const encoder = new TextEncoder();

          let currentArgvBuf = argvBuf;
          for (let i = 0; i < args.length; i++) {
            view.setUint32(argv + i * 4, currentArgvBuf, true);

            const encoded = encoder.encode(args[i] + '\0');
            memoryBytes.set(encoded, currentArgvBuf);
            currentArgvBuf += encoded.length;
          }
          return 0;
        };

        // Override prestat to respect opts.cwd or default to /
        const cwd = opts.cwd || '/';

        const originalFdPrestatGet = wasi.fd_prestat_get.actual || wasi.fd_prestat_get;
        wasi.fd_prestat_get.actual = function (fd, bufPtr) {
          if (fd === 3) {
            const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
            const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);
            const view = new DataView(memory.buffer);
            view.setUint8(bufPtr, 0); // PREOPENTYPE_DIR
            view.setUint32(bufPtr + 4, cwd.length, true);
            return 0;
          }
          return 8; // EBADF
        };

        const originalFdPrestatDirName = wasi.fd_prestat_dir_name.actual || wasi.fd_prestat_dir_name;
        wasi.fd_prestat_dir_name.actual = function (fd, pathPtr, pathLen) {
          if (fd === 3) {
            const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
            const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);
            const memoryBytes = new Uint8Array(memory.buffer);
            const encoder = new TextEncoder();
            const encoded = encoder.encode(cwd);
            if (pathLen < encoded.length) return 63; // ENAMETOOLONG
            memoryBytes.set(encoded, pathPtr);
            return 0;
          }
          return 28; // EINVAL
        };

        // Override environ to pass PWD and others
        const env = { ...opts.env, PWD: cwd };
        const envPairs = Object.entries(env).map(([k, v]) => `${k}=${v}`);

        const originalEnvironSizesGet = wasi.environ_sizes_get.actual || wasi.environ_sizes_get;
        wasi.environ_sizes_get.actual = function (environc, environBufSize) {
          const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
          const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);
          if (!memory) throw new Error('WASI environ_sizes_get: Memory not found');
          const view = new DataView(memory.buffer);

          view.setUint32(environc, envPairs.length, true);
          let totalSize = 0;
          for (const pair of envPairs) {
            totalSize += new TextEncoder().encode(pair).length + 1;
          }
          view.setUint32(environBufSize, totalSize, true);
          return 0;
        };

        const originalEnvironGet = wasi.environ_get.actual || wasi.environ_get;
        wasi.environ_get.actual = function (environ, environBuf) {
          const inst = (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.instance) ? globalThis.__washm_bootstrap.instance : null;
          const memory = (this && this.exports) ? this.exports.memory : (inst ? inst.exports.memory : null);
          if (!memory) throw new Error('WASI environ_get: Memory not found');

          const view = new DataView(memory.buffer);
          const memoryBytes = new Uint8Array(memory.buffer);
          const encoder = new TextEncoder();

          let currentBuf = environBuf;
          for (let i = 0; i < envPairs.length; i++) {
            view.setUint32(environ + i * 4, currentBuf, true);
            const encoded = encoder.encode(envPairs[i] + '\0');
            memoryBytes.set(encoded, currentBuf);
            currentBuf += encoded.length;
          }
          return 0;
        };
      }

      // Use real global WebAssembly
      return realGlobal.WebAssembly.instantiate(buffer, importObject);
    }
  };

  // Global scope self-reference
  // Mock washm_tty with robust read implementation
  var washm_tty = {
    read: () => new Promise(resolve => {
      if (inputQueue.length > 0) {
        resolve(inputQueue.shift());
      } else {
        inputNotify = resolve;
      }
    }),
    enableRaw: () => ({ ok: true }),
    disableRaw: () => ({ ok: true }),
    size: () => ({ rows: 24, cols: 80 })
  };

  const globalThis = {
    ...window,
    console,
    WebAssembly,
    fetch,
    window,
    document,
    navigator,
    washm_tty
  };
  globalThis.globalThis = globalThis;

  // Mounted VFS (needed for protocol.js)
  const mounted = {
    washm: { start_pwd: opts.cwd || '/' }
  };

  // ==========================================================================
  // Prepare Code for Eval
  // ==========================================================================

  // Inject bootstrap hook by prepending it to washm.js
  // This ensures it shares the same scope (closure) as washm.js body
  const bootstrapHookCode = `
    globalThis.bootstrapEval = function(js, ctxInstance) {
      // Skip vendor code that causes issues in mock environment
      if (js.includes('node_modules/xterm') || 
          js.includes('xterm.css') || 
          js.includes('xterm-addon-fit')) {
          return;
      }
      try {
        // Shadow 'instance' with the one passed from context (local or global)
        var instance = ctxInstance;
        // console.log('Bootstrap instance:', !!instance);
        
        // Direct eval runs in this scope (washm.js scope + local vars)
        eval(js); 
      } catch(e) {
        console.error('Bootstrap eval error:', e);
      }
    };
  `;

  // Modify washm.js to use the hook and pass 'instance' available in that scope
  washmJs = bootstrapHookCode + washmJs.replace(/eval\(js\);/g, 'globalThis.bootstrapEval(js, typeof instance !== "undefined" ? instance : undefined);');

  // Eval washm.js in this scope
  // It will see window, document, etc. (defined above)
  try {
    // eslint-disable-next-line no-eval
    eval(washmJs);
  } catch (err) {

    // washm.js browser mode is async mostly. It might not throw.
    console.error('Washm eval error:', err);
  }

  // washm.js browser mode doesn't exit. It starts the terminal and WASM.
  // We need to wait for exit?
  // Our tests execute commands.
  // browser mode executes `_start` which runs the shell or command.
  // If it's a command (echo), it runs and exits. 
  // BUT washm.js browser mode DOES NOT call `proc_exit`? 
  // It catches `proc_exit` in `mainBrowser`? 
  // No, `mainBrowser` catches the promise rejection.

  // We need to capture the exit code.
  // `proc_exit` throws Error('WASI proc_exit: code').
  // `mainBrowser` calls `_start`.
  // If `_start` throws, `mainBrowser` catches it.

  // We need to hook `proc_exit` or `mainBrowser` error handling.
  // `washm.js` defines:
  // globalThis.washm_wasi_overrides = {}

  // We can hook proc_exit via overrides!
  // BUT `washm.js` browser mode `proc_exit` implementation just throws.
  // Check `washm.js`:
  /*
      proc_exit: function(code) {
        console.log('Process exit with code:', code);
        throw new Error('WASI proc_exit: ' + code);
      },
  */
  // And it is NOT one of the overridable functions in the list (fd_write, fd_close, random_get, clock_time_get).
  // Wait, let me check `washm.js` again for `proc_exit` override support.

  return {
    streams,
    get exitCode() { return exitCode; },
    waitForExit: async () => {
      // We might need to wait until exitCode is set.
      // Polling for now as a fallback?
      // Or better, hook the exit.
      await new Promise(r => setTimeout(r, 100));
      return exitCode;
    }
  };
}

// ============================================================================
// Unified API
// ============================================================================

/**
 * Creates a unified test environment for washm
 * @param {Object} opts - Configuration options
 * @param {string} opts.mode - 'node' or 'browser' (default: 'node')
 * @param {string} opts.cwd - Initial working directory (default: '/')
 * @param {Object} opts.env - Environment variables
 * @param {string} opts.platform - process.platform (node mode)
 * @param {string} opts.arch - process.arch (node mode)
 * @param {number} opts.cpus - navigator.hardwareConcurrency (browser mode)
 * @param {number} opts.memory - navigator.deviceMemory (browser mode)
 */
export async function washEnvironment(opts = {}) {
  const mode = opts.mode || 'node';

  // Create stream buffers
  const streams = {
    stdin: new MockStdin(),
    stdout: new MockStdout(),
    stderr: new MockStdout(),
  };

  return {
    mode,
    streams,
    washm: async function (args = []) {
      let runtime;
      if (mode === 'node') {
        runtime = await nodePreloader({ ...opts, args }, streams);
      } else if (mode === 'browser') {
        runtime = await browserPreloader({ ...opts, args }, streams);
      } else {
        throw new Error(`Unknown mode: ${mode}. Use 'node' or 'browser'`);
      }
      return new WashmInstance(runtime, streams);
    }
  };
}


// Export classes for testing
export { MockStdin, MockStdout };
