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

  write(str) {
    this.chunks.push(str);
  }

  readAll() {
    return this.chunks.join('');
  }

  clear() {
    this.chunks = [];
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
// Unified API
// ============================================================================

/**
 * Create a mock WASM environment for testing
 * @param {Object} opts - Configuration options
 * @param {string} [opts.mode='node'] - Execution mode: 'node' or 'browser'
 * @param {string} [opts.cwd='/'] - Initial working directory
 * @param {Record<string, string>} [opts.env] - Environment variables
 * @returns {Promise<Object>} Mock environment interface
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
      if (mode === 'node') {
        // Use Node preloader - pass args through opts
        return await nodePreloader({ ...opts, args }, streams);
      } else {
        // Browser mode not implemented yet
        return {
          streams,
          waitForExit: async () => { },
          exitCode: 0
        };
      }
    }
  };
}

// Export classes for testing
export { MockStdin, MockStdout };
