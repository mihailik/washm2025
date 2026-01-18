// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { washEnvironment } from './washm_mock.js';

test('washEnvironment loads without errors', async () => {
  const env = await washEnvironment({ mode: 'node' });
  assert.ok(env, 'Environment should be created');
  assert.equal(env.mode, 'node', 'Mode should be set to node');
  assert.equal(typeof env.washm, 'function', 'washm should be a function');
});

test('washEnvironment accepts browser mode', async () => {
  const env = await washEnvironment({ mode: 'browser' });
  assert.ok(env, 'Environment should be created');
  assert.equal(env.mode, 'browser', 'Mode should be set to browser');
});

test('washEnvironment returns mock washm instance', async () => {
  const env = await washEnvironment();
  const washm = await env.washm([]);

  assert.ok(washm.streams, 'Should have streams');
  assert.ok(washm.streams.stdin, 'Should have stdin');
  assert.ok(washm.streams.stdout, 'Should have stdout');
  assert.ok(washm.streams.stderr, 'Should have stderr');
  assert.equal(typeof washm.waitForExit, 'function', 'Should have waitForExit');
  assert.equal(typeof washm.exitCode, 'number', 'Should have exitCode');
});
