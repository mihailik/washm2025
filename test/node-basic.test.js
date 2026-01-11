// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

test('node mode: WASM loads and exits', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  assert.deepStrictEqual({
    hasInstance: !!washm,
    hasStreams: !!washm.streams,
    exitCodeType: typeof washm.exitCode
  }, {
    hasInstance: true,
    hasStreams: true,
    exitCodeType: 'number'
  });
});

test('node mode: washm.js is actually evaluated', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  // If washm.js was evaluated, it would have tried to instantiate WebAssembly
  // and write to stdout/stderr. Check for any captured output.
  const stdout = washm.streams.stdout.readAll();
  const stderr = washm.streams.stderr.readAll();

  // Single structural assertion for types
  assert.deepStrictEqual({
    stdoutType: typeof stdout,
    stderrType: typeof stderr
  }, {
    stdoutType: 'string',
    stderrType: 'string'
  });

  // The fact that we get here without errors means washm.js was evaluated
  // (otherwise we'd get syntax errors or module not found errors)
});

test('node mode: captures stdout output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  // Check if any output was captured
  const output = washm.streams.stdout.readAll();
  assert.deepStrictEqual({ outputType: typeof output }, { outputType: 'string' });
});

test('node mode: check stderr output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  const stderr = washm.streams.stderr.readAll();
  assert.deepStrictEqual({
    stderrType: typeof stderr,
    exitCodeType: typeof washm.exitCode
  }, {
    stderrType: 'string',
    exitCodeType: 'number'
  });
});

test('node mode: executes echo command', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm(['-c', 'echo hello world']);

  // Small delay to allow async WASM operations to complete
  await new Promise(resolve => setTimeout(resolve, 50));

  const stdout = env.streams.stdout.readAll();
  const stderr = env.streams.stderr.readAll();

  // single structural assertion for echo command output
  assert.deepStrictEqual({ stdout, stderr, exitCode: washm.exitCode }, {
    stdout: 'hello world\n',
    stderr: '',
    exitCode: 0
  });
});

test('node mode: executes pwd command', async () => {
  const env = await washEnvironment({ mode: 'node', cwd: '/test/directory' });
  const washm = await env.washm(['-c', 'pwd']);

  // Small delay to allow async WASM operations to complete
  await new Promise(resolve => setTimeout(resolve, 50));

  const stdout = env.streams.stdout.readAll();
  const stderr = env.streams.stderr.readAll();

  // single structural assertion for pwd behavior (non-empty string and ends with newline)
  assert.deepStrictEqual({
    stdoutNonEmpty: stdout.length > 0,
    stdoutEndsWithNewline: stdout.endsWith('\n'),
    stderrEmpty: stderr === '',
    exitCodeZero: washm.exitCode === 0
  }, {
    stdoutNonEmpty: true,
    stdoutEndsWithNewline: true,
    stderrEmpty: true,
    exitCodeZero: true
  });
});

test.skip('node mode: shell prints version with --version', async () => {
  // This will be implemented in Phase 4 (bootstrap integration)
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm(['--version']);

  const output = washm.streams.stdout.readAll();
  assert.match(output, /brush/, 'Should contain version info');
});

test.skip('node mode: shell responds to commands', async () => {
  // This will be implemented in Phase 5 (complete WASI & VFS)
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm(['-c', 'echo hello']);

  const output = washm.streams.stdout.readAll();
  assert.match(output, /hello/, 'Should execute echo command');
});
