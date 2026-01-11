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
    cpus: () => [{ model: 'mock-cpu' }],
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
function createMockRequire(mockFs, mockOs, mockTty, mockChildProcess) {
  return (moduleName) => {
    switch (moduleName) {
      case 'fs': return mockFs;
      case 'os': return mockOs;
      case 'tty': return mockTty;
      case 'child_process': return mockChildProcess;
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

  // Placeholder implementation - will be expanded in future phases
  return {
    mode,
    streams,
    washm: async function (args = []) {
      return {
        streams,
        waitForExit: async () => { },
        exitCode: 0
      };
    }
  };
}

// Export classes for testing
export { MockStdin, MockStdout };
