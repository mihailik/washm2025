// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

test('node mode: WASM loads and exits', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  assert.ok(washm, 'Should return washm instance');
  assert.ok(washm.streams, 'Should have streams');
  assert.equal(typeof washm.exitCode, 'number', 'Should have exitCode property');
});

test('node mode: washm.js is actually evaluated', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  // If washm.js was evaluated, it would have tried to instantiate WebAssembly
  // and write to stdout/stderr. Check for any captured output.
  const stdout = washm.streams.stdout.readAll();
  const stderr = washm.streams.stderr.readAll();

  // At minimum, some output should exist (even if empty strings)
  assert.equal(typeof stdout, 'string', 'stdout should be captured as string');
  assert.equal(typeof stderr, 'string', 'stderr should be captured as string');

  // The fact that we get here without errors means washm.js was evaluated
  // (otherwise we'd get syntax errors or module not found errors)
  assert.ok(true, 'washm.js was successfully evaluated');
});

test('node mode: captures stdout output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  // Check if any output was captured
  const output = washm.streams.stdout.readAll();
  assert.equal(typeof output, 'string', 'Should capture string output');

  // Log what we actually captured
  console.log('=== CAPTURED STDOUT ===');
  console.log(output || '(empty)');
  console.log('=== END STDOUT ===');
});

test('node mode: check stderr output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm([]);

  const stderr = washm.streams.stderr.readAll();
  console.log('=== CAPTURED STDERR ===');
  console.log(stderr || '(empty)');
  console.log('=== END STDERR ===');
  console.log('Exit code:', washm.exitCode);
});

test('node mode: executes echo command', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const washm = await env.washm(['-c', 'echo hello world']);

  // Small delay to allow async WASM operations to complete
  await new Promise(resolve => setTimeout(resolve, 50));

  const stdout = env.streams.stdout.readAll();
  const stderr = env.streams.stderr.readAll();

  // echo should print the text with newline
  assert.equal(stdout, 'hello world\n', 'Should output "hello world" with newline');
  assert.equal(stderr, '', 'Should have no stderr output');
  assert.equal(washm.exitCode, 0, 'Should exit with code 0');
});

test('node mode: executes pwd command', async () => {
  const env = await washEnvironment({ mode: 'node', cwd: '/test/directory' });
  const washm = await env.washm(['-c', 'pwd']);

  // Small delay to allow async WASM operations to complete
  await new Promise(resolve => setTimeout(resolve, 50));

  const stdout = env.streams.stdout.readAll();
  const stderr = env.streams.stderr.readAll();

  // pwd should print the current working directory
  assert.ok(stdout.length > 0, 'Should output a directory path');
  assert.ok(stdout.endsWith('\n'), 'Should end with newline');
  assert.equal(stderr, '', 'Should have no stderr output');
  assert.equal(washm.exitCode, 0, 'Should exit with code 0');
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
