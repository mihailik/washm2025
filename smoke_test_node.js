#!/usr/bin/env node
'use strict';

const { promisify } = require('util');
const exec = promisify(require('child_process').exec);
const fsPromises = require('fs').promises;
const fs = require('fs');
const path = require('path');

async function buildWasm() {
  console.log('Building WASM (wasm32-wasip1)...');
  try { await exec('rustup target add wasm32-wasip1'); } catch (e) { }
  await exec('cargo build --target wasm32-wasip1');
}

async function waitForJson(dir, timeoutMs=10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const files = await fsPromises.readdir(dir);
      const json = files.find(f => f.endsWith('.json'));
      if (json) return path.join(dir, json);
    } catch(e) {}
    await new Promise(r => setTimeout(r, 200));
  }
  throw new Error('timeout waiting for json');
}

(async () => {
  try {
    // build
    await buildWasm();

    const wasm = path.join('target','wasm32-wasip1','debug','washm.wasm');
    if (!fs.existsSync(wasm)) throw new Error('compiled wasm not found');

    // start host in-process via require
    const { startHostInstance } = require('./host/node-host');
    const host = await startHostInstance(wasm);

    // wait for result
    const j = await waitForJson(path.resolve('host_eval'));
    console.log('JSON result produced at', j);
    const content = await fsPromises.readFile(j, 'utf8');
    console.log(content);

    await host.stop();
    process.exit(0);
  } catch (e) {
    console.error('smoke test failed', e);
    process.exit(1);
  }
})();
