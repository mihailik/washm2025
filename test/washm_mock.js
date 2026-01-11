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

  // Placeholder implementation - will be expanded in future phases
  return {
    mode,
    washm: async function (args = []) {
      return {
        streams: {
          stdin: { write: () => { }, end: () => { } },
          stdout: { readAll: () => '' },
          stderr: { readAll: () => '' }
        },
        waitForExit: async () => { },
        exitCode: 0
      };
    }
  };
}
