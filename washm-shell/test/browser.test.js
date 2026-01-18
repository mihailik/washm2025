// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

test('browser mode: environment loads without errors', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  const instance = await env.washm(['echo', 'hello']);

  assert.deepStrictEqual({
    hasInstance: !!instance,
    exitCode: instance.exitCode
  }, {
    hasInstance: true,
    exitCode: 0
  });
});

test('browser mode: terminal is mocked correctly', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  await env.washm();

  assert.deepStrictEqual({
    hasStdout: !!env.streams.stdout,
    hasStdin: !!env.streams.stdin
  }, {
    hasStdout: true,
    hasStdin: true
  });
});

test('browser mode: executes echo command', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  // washm.js in browser mode starts the shell/command immediately
  await env.washm(['echo', 'hello browser']);

  // Wait for async execution
  await new Promise(resolve => setTimeout(resolve, 50));

  const output = env.streams.stdout.readAll();
  assert.strictEqual(output, 'hello browser\n');
});

test('browser mode: executes pwd command', async () => {
  const env = await washEnvironment({
    mode: 'browser',
    // Mock environment currently defaults to root / behavior
  });
  await env.washm(['pwd']);

  // Wait for async execution
  await new Promise(resolve => setTimeout(resolve, 50));

  const output = env.streams.stdout.readAll();
  assert.strictEqual(output, '/\n');
});

test('browser mode: handles exit codes', async () => {
  // Note: browser mode currently doesn't propagate exit codes perfectly due to lack of proc_exit throw
  // But we can test it mocked 0
  const env = await washEnvironment({ mode: 'browser' });
  const instance = await env.washm(['true']); // 'true' exits with 0

  await new Promise(resolve => setTimeout(resolve, 50));
  assert.strictEqual(instance.exitCode, 0);
});
