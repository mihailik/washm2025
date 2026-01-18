// @ts-check
import { test } from 'node:test';
import assert from 'node:assert';
import { MockStdin, MockStdout } from './washm_mock.js';

test('MockStdin can buffer input', () => {
  const stdin = new MockStdin();

  stdin.write('hello\n');
  stdin.write('world\n');

  assert.equal(stdin.read(), 'hello\n');
  assert.equal(stdin.read(), 'world\n');
  assert.equal(stdin.read(), null);
});

test('MockStdin can be closed', () => {
  const stdin = new MockStdin();

  stdin.write('test');
  stdin.end();

  assert.equal(stdin.closed, true);
  assert.throws(() => stdin.write('more'), /Cannot write to closed stdin/);
});

test('MockStdin read returns null when empty', () => {
  const stdin = new MockStdin();

  assert.equal(stdin.read(), null);
  assert.equal(stdin.read(), null);
});

test('MockStdout can capture output', () => {
  const stdout = new MockStdout();

  stdout.write('Hello ');
  stdout.write('World');
  stdout.write('!\n');

  assert.equal(stdout.readAll(), 'Hello World!\n');
});

test('MockStdout readAll concatenates all chunks', () => {
  const stdout = new MockStdout();

  for (let i = 0; i < 5; i++) {
    stdout.write(`line ${i}\n`);
  }

  const output = stdout.readAll();
  assert.match(output, /line 0/);
  assert.match(output, /line 4/);
  assert.equal(output.split('\n').length - 1, 5); // 5 newlines
});

test('MockStdout can be cleared', () => {
  const stdout = new MockStdout();

  stdout.write('first');
  assert.equal(stdout.readAll(), 'first');

  stdout.clear();
  assert.equal(stdout.readAll(), '');

  stdout.write('second');
  assert.equal(stdout.readAll(), 'second');
});

test('MockStdout starts empty', () => {
  const stdout = new MockStdout();
  assert.equal(stdout.readAll(), '');
});
