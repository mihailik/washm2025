// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

// ============================================================================
// Step 8: Cleanup and Teardown Tests
// ============================================================================

test('node mode: cleanup is called after waitForExit', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['echo', 'test']);

  // Verify instance has cleanup tracking
  assert.strictEqual(instance._cleaned, false);

  await instance.waitForExit();

  // Verify cleanup was called
  assert.strictEqual(instance._cleaned, true);
});

test('browser mode: cleanup is called after waitForExit', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  const instance = await env.washm(['echo', 'test']);

  assert.strictEqual(instance._cleaned, false);

  await instance.waitForExit();

  assert.strictEqual(instance._cleaned, true);
});

test('cleanup can be called multiple times without error', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['true']);

  await instance.waitForExit();

  // Call cleanup explicitly again
  await instance.cleanup();
  await instance.cleanup();

  // Should still be marked as cleaned
  assert.strictEqual(instance._cleaned, true);
});

test('streams remain accessible after cleanup for inspection', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['echo', 'preserved']);

  await new Promise(resolve => setTimeout(resolve, 50));
  await instance.waitForExit();

  // Streams should still be readable after cleanup
  // Access through env.streams since that's the shared reference
  assert.strictEqual(env.streams.stdout.readAll(), 'preserved\n');
});

test('manual cleanup: streams can be cleared explicitly', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['echo', 'to be cleared']);

  await new Promise(resolve => setTimeout(resolve, 50));
  await instance.waitForExit();

  const output = env.streams.stdout.readAll();
  assert.strictEqual(output, 'to be cleared\n');

  // Clear streams manually
  env.streams.stdout.clear();
  env.streams.stderr.clear();

  assert.strictEqual(env.streams.stdout.readAll(), '');
  assert.strictEqual(env.streams.stderr.readAll(), '');
});

test('cleanup without waitForExit can be called directly', async () => {
  const env = await washEnvironment({ mode: 'node' });
  const instance = await env.washm(['true']);

  await new Promise(resolve => setTimeout(resolve, 50));

  // Call cleanup directly without waitForExit
  await instance.cleanup();

  assert.strictEqual(instance._cleaned, true);
});
