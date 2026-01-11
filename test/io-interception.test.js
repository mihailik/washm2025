// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

// ============================================================================
// Step 4 & 5: I/O Interception API Tests
// ============================================================================

test('node mode: stdout interception captures echo output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  await env.washm(['echo', 'test output']);
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.strictEqual(env.streams.stdout.readAll(), 'test output\n');
});

test('node mode: stderr interception captures error output', async () => {
  const env = await washEnvironment({ mode: 'node' });
  // Use a command that writes to stderr (e.g., a non-existent command or false)
  await env.washm(['sh', '-c', 'echo error >&2']);
  await new Promise(resolve => setTimeout(resolve, 50));

  const stderr = env.streams.stderr.readAll();
  assert.ok(stderr.length > 0);
});

test('browser mode: stdout interception captures echo output', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  await env.washm(['echo', 'browser output']);
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.strictEqual(env.streams.stdout.readAll(), 'browser output\n');
});

test('browser mode: multiple commands produce sequential output', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  await env.washm(['echo', 'first']);
  await new Promise(resolve => setTimeout(resolve, 30));

  const output1 = env.streams.stdout.readAll();
  env.streams.stdout.clear();

  await env.washm(['echo', 'second']);
  await new Promise(resolve => setTimeout(resolve, 30));

  const output2 = env.streams.stdout.readAll();

  assert.deepStrictEqual({ output1, output2 }, {
    output1: 'first\n',
    output2: 'second\n'
  });
});

test('unified API: WashmInstance provides consistent exitCode access', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['true']);
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.deepStrictEqual({
    hasStreams: !!instance.streams,
    hasExitCode: typeof instance.exitCode === 'number',
    exitCode: instance.exitCode
  }, {
    hasStreams: true,
    hasExitCode: true,
    exitCode: 0
  });
});

test('unified API: streams are accessible from both env and instance', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['echo', 'test']);
  await new Promise(resolve => setTimeout(resolve, 50));

  // Both should reference the same streams
  assert.strictEqual(env.streams, instance.streams);
  assert.strictEqual(env.streams.stdout.readAll(), 'test\n');
});

test('unified API: mode parameter switches between node and browser', async () => {
  const nodeEnv = await washEnvironment({ mode: 'node' });
  const browserEnv = await washEnvironment({ mode: 'browser' });

  assert.deepStrictEqual({
    nodeMode: nodeEnv.mode,
    browserMode: browserEnv.mode
  }, {
    nodeMode: 'node',
    browserMode: 'browser'
  });
});

test('I/O interception: stdout can be cleared between commands', async () => {
  const env = await washEnvironment({ mode: 'node' });
  await env.washm(['echo', 'first']);
  await new Promise(resolve => setTimeout(resolve, 50));

  const first = env.streams.stdout.readAll();
  env.streams.stdout.clear();

  await env.washm(['echo', 'second']);
  await new Promise(resolve => setTimeout(resolve, 50));

  const second = env.streams.stdout.readAll();

  assert.deepStrictEqual({ first, second }, {
    first: 'first\n',
    second: 'second\n'
  });
});

test('I/O interception: empty streams return empty strings', async () => {
  const env = await washEnvironment({ mode: 'node' });
  await env.washm(['true']); // Command that produces no output
  await new Promise(resolve => setTimeout(resolve, 50));

  assert.deepStrictEqual({
    stdout: env.streams.stdout.readAll(),
    stderr: env.streams.stderr.readAll()
  }, {
    stdout: '',
    stderr: ''
  });
});
