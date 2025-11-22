#!/usr/bin/env node
// @ts-check

/* Node host for WASI Async interop. Implements the /dev/host/eval protocol + _continue.

   This host uses `node:wasi` and expects a WASM guest compiled for `wasm32-wasi`.
*/

const fs = require('fs/promises');
const path = require('path');
const { WASI } = require('wasi');
const { TextDecoder, TextEncoder } = require('util');

// Expose require and process globally for AsyncFunction access
global.require = require;
global.process = process;

// `/dev/host/eval` is virtual-only; no host directory mapping is allowed.

async function run() {

  // support a `--check` flag to print module summary before instantiating
  const cliArgs = process.argv.slice(2);
  const hasCheck = cliArgs.includes('--check');
  const checkOnly = cliArgs.includes('--check-only');
  // detect wasm path as the first non-flag argument; otherwise we'll use defaults later
  let wasmPath = cliArgs.find(a => !a.startsWith('-'));
  // If argv[2] is missing or looks like a flag (starts with -), assume it's not the wasm file
  if (!wasmPath || wasmPath.startsWith('-')) {
    // Try release first as it is likely more up-to-date in this workflow
    wasmPath = path.resolve(__dirname, 'target/wasm32-wasip1/release/washm.wasm');
    try {
      await fs.access(wasmPath);
    } catch (e) {
      // Fallback to debug
      wasmPath = path.resolve(__dirname, 'target/wasm32-wasip1/debug/washm.wasm');
      try {
        await fs.access(wasmPath);
      } catch (e) {
        // Fallback to local file
        wasmPath = path.resolve(__dirname, 'washm.wasm');
        try {
          await fs.access(wasmPath);
        } catch (e) {
          console.error('usage: node axy.js <guest.wasm>');
          process.exit(2);
          return;
        }
      }
    }
  }

  // By spec, /dev/host/eval must not be preopened to a real host path; the host will implement an in-memory VFS overlay.
  // Build guest args for WASI: use wasm basename as argv[0], then any args after the wasm path
  const cliArgsFiltered = cliArgs.filter(a => a !== '--check' && a !== '--check-only');
  let guestArgs = [];
  if (wasmPath) {
    const wasmIndex = cliArgsFiltered.indexOf(wasmPath);
    const trailing = wasmIndex >= 0 ? cliArgsFiltered.slice(wasmIndex + 1) : cliArgsFiltered;
    guestArgs = [path.basename(wasmPath), ...trailing];
  } else {
    // No wasm path provided, leave guestArgs empty
    guestArgs = [];
  }
  const wasiArgs = guestArgs;
  const wasi = new WASI({
    args: wasiArgs,
    preopens: { [process.cwd()]: process.cwd() },
    version: 'preview1'
  });
  const wasm = await fs.readFile(wasmPath);
  // If check flag was passed, print a summary before continuing
  if (hasCheck) {
    try {
      printWasmSummary(wasmPath, wasm);
    } catch (err) {
      console.error('Failed to parse WASM for check:', err);
    }
    if (checkOnly) {
      // Exit early: user only wanted to check the WASM, don't instantiate.
      return;
    }
  }
  const module = await WebAssembly.compile(wasm);
  const importObj = { wasi_snapshot_preview1: wasi.wasiImport };

  // Setup virtual vfs
  const evalVfs = new Map();
  const fdToVfs = new Map();
  let nextFakeFd = 1000;
  const memRef = { memory: null };
  const orig = wasi.wasiImport;
  const wrapped = { ...orig };

  function readStringFromMemory(ptr, len) {
    const mem = memRef.memory;
    if (!mem) return '';
    return new TextDecoder('utf-8').decode(new Uint8Array(mem.buffer, ptr, len));
  }
  function writeU32(ptr, val) {
    new DataView(memRef.memory.buffer).setUint32(ptr, val, true);
  }

  wrapped.path_open = function (dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr) {
    const p = readStringFromMemory(path_ptr, path_len);
    if (p.startsWith('/dev/host/eval')) {
      const fd = nextFakeFd++;
      fdToVfs.set(fd, { path: p, pos: 0, mode: 'rw' });
      if (!evalVfs.has(p)) evalVfs.set(p, { js: '', json: null });
      try { writeU32(fd_out_ptr, fd); } catch (e) {}
      return 0;
    }
    return orig.path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };
  wrapped.fd_write = function (fd, iovs_ptr, iovs_len, nwritten_ptr) {
    if (fdToVfs.has(fd)) {
      const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      for (let i = 0; i < iovs_len; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const bytes = new Uint8Array(mem.buffer, bufPtr, bufLen);
        const text = new TextDecoder('utf-8').decode(bytes);
        const entry = evalVfs.get(fdToVfs.get(fd).path) || { js: '', json: null };
        entry.js += text; evalVfs.set(fdToVfs.get(fd).path, entry);
        total += bufLen;
      }
      try { writeU32(nwritten_ptr, total); } catch (e) {}
      return 0;
    }
    return orig.fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr);
  };
  wrapped.fd_read = function (fd, iovs_ptr, iovs_len, nread_ptr) {
    if (fdToVfs.has(fd)) {
      const entry = evalVfs.get(fdToVfs.get(fd).path) || { js: '', json: null };
      const readBuf = entry.json === null ? '' : entry.json; const mem = memRef.memory; const dv = new DataView(mem.buffer);
      let total = 0;
      for (let i = 0; i < iovs_len && total < readBuf.length; i++) {
        const off = iovs_ptr + i * 8; const bufPtr = dv.getUint32(off, true); const bufLen = dv.getUint32(off + 4, true);
        const toWrite = Math.min(bufLen, readBuf.length - total);
        const bytes = new TextEncoder().encode(readBuf.slice(total, total + toWrite));
        new Uint8Array(mem.buffer, bufPtr, toWrite).set(bytes);
        total += toWrite;
      }
      try { writeU32(nread_ptr, total); } catch (e) {}
      return 0;
    }
    return orig.fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  };
  wrapped.fd_close = function (fd) {
    if (fdToVfs.has(fd)) {
      const info = fdToVfs.get(fd); fdToVfs.delete(fd);
      if (info.path.endsWith('.js')) {
        const cookie = path.basename(info.path, '.js'); const entry = evalVfs.get(info.path) || { js: '', json: null };
        const jsText = entry.js;
        (async () => {
          let result;
          try {
            let isAsync = false;
            try { new Function(jsText); } catch (e) { if (e instanceof SyntaxError) isAsync = true; else throw e; }
            if (isAsync) { const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor; const fn = new AsyncFunction(jsText); result = await fn(); }
            else result = eval(jsText);
            if (result instanceof Promise) result = await result;
          } catch (err) { result = { error: String(err) }; }
          entry.json = JSON.stringify({ ok: true, result }); evalVfs.set(`/dev/host/eval/${cookie}.json`, entry);
          if (typeof instance.exports._continue === 'function') instance.exports._continue(Number(cookie));
        })();
      }
      return 0;
    }
    return orig.fd_close(fd);
  };

  importObj.wasi_snapshot_preview1 = wrapped;

  const instance = await WebAssembly.instantiate(module, importObj);
  memRef.memory = instance.exports.memory;

  // Reactor initialization
  // wasi.initialize(instance) will call _initialize export if it exists.
  try {
    wasi.initialize(instance);
  } catch (e) {
    // Ignore if already initialized or if it fails safely (e.g. no _initialize)
    // But for random_get to work, wasi must be initialized.
  }
}

/**
 * Read an unsigned LEB128 varuint32 from bytes at the offset.
 * @param {Uint8Array} bytes
 * @param {number} offset
 * @returns {{value:number, next:number}}
 */
function readVarUint32U8(bytes, offset) {
  let result = 0;
  let shift = 0;
  let pos = offset;
  while (true) {
    if (pos >= bytes.length) throw new Error('Unexpected EOF while reading varuint');
    const byte = bytes[pos++];
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) break;
    shift += 7;
  }
  return { value: result >>> 0, next: pos };
}

/**
 * @param {Uint8Array|Buffer} bytes
 */
function parseCustomAndStartSections(bytes) {
  const u8 = new Uint8Array(bytes);
  const customSections = [];
  let startSectionPresent = false;
  // wasm header is 8 bytes
  let offset = 8;
  while (offset < u8.length) {
    const id = u8[offset++];
    const ret = readVarUint32U8(u8, offset);
    const sectionLen = ret.value; offset = ret.next;
    const sectionStart = offset;
    if (id === 0) { // custom
      const nameLenRet = readVarUint32U8(u8, offset);
      const nameLen = nameLenRet.value; let namePtr = nameLenRet.next;
      const nameBytes = u8.slice(namePtr, namePtr + nameLen);
      const name = new TextDecoder('utf-8').decode(nameBytes);
      customSections.push({ name, offset: sectionStart, length: sectionLen });
    } else if (id === 8) {
      startSectionPresent = true;
    }
    offset = sectionStart + sectionLen;
  }
  return { customSections, startSectionPresent };
}

/**
 * @param {Uint8Array|Buffer} payload
 */
function parseNameSectionFunctions(payload) {
  // payload is Uint8Array bytes
  try {
    const u8 = payload instanceof Uint8Array ? payload : new Uint8Array(payload);
    let offset = 0;
    const functions = [];
    while (offset < u8.length) {
      const subId = u8[offset++];
      const lenRet = readVarUint32U8(u8, offset);
      const subLen = lenRet.value; offset = lenRet.next;
      if (subId === 1) { // function names
        // vector of name associations
        const vecLenRet = readVarUint32U8(u8, offset); offset = vecLenRet.next; const vecLen = vecLenRet.value;
        for (let i = 0; i < vecLen; i++) {
          const idxRet = readVarUint32U8(u8, offset); const idx = idxRet.value; offset = idxRet.next;
          const strLenRet = readVarUint32U8(u8, offset); const sLen = strLenRet.value; offset = strLenRet.next;
          const strBytes = u8.slice(offset, offset + sLen);
          const name = new TextDecoder('utf-8').decode(strBytes);
          functions.push({ idx, name });
          offset += sLen;
        }
      } else {
        // skip unknown subsection
        offset += subLen;
      }
    }
    return functions;
  } catch (e) {
    return [];
  }
}

/**
 * @param {string} wasmPath
 * @param {Buffer|Uint8Array} wasmBuffer
 */
function printWasmSummary(wasmPath, wasmBuffer) {
  const statInfo = { size: wasmBuffer.length };
  console.log(`WASM CHECK: ${wasmPath} (${statInfo.size} bytes)`);
  const u8buf = new Uint8Array(wasmBuffer);
  console.log('Valid:', WebAssembly.validate(u8buf));

  let module;
  try {
    module = new WebAssembly.Module(u8buf);
  } catch (err) {
    console.error('Failed to compile module for introspection:', String(err));
    return;
  }

  const imports = WebAssembly.Module.imports(module) || [];
  const exports = WebAssembly.Module.exports(module) || [];

  const importedModules = Object.create(null);
  imports.forEach(i => { importedModules[i.module] = importedModules[i.module] || []; importedModules[i.module].push(i); });
  console.log('Imports total:', imports.length);
  Object.keys(importedModules).forEach(m => {
    console.log(`  module: ${m} (${importedModules[m].length})`);
    /** @param {{name:string, kind:string}} imp */
    for (let j = 0; j < importedModules[m].length; j++) {
      const imp = importedModules[m][j];
      console.log(`    ${imp.name} : ${imp.kind}`);
    }
  });

  console.log('Exports total:', exports.length);
  exports.forEach(e => console.log(`  ${e.name} : ${e.kind}`));

  // parse custom sections & start
  const { customSections, startSectionPresent } = parseCustomAndStartSections(wasmBuffer);
  console.log('Start section present:', startSectionPresent);
  console.log('Custom sections:');
  // limit to first 7 custom sections
  const customLimit = 7;
  customSections.slice(0, customLimit).forEach(s => {
    console.log(`  ${s.name} (offset: ${s.offset}, len: ${s.length})`);
    try {
      const raw = WebAssembly.Module.customSections(module, s.name) || [];
      if (raw.length) {
        const snippet = snippetForCustomSection(new Uint8Array(raw[0]));
        console.log(`    sample: ${snippet}`);
      }
    } catch (_) {}
  });
  if (customSections.length > customLimit) console.log(`  ... and ${customSections.length - customLimit} more`);

  // if name section present, decode function names
  const nameSections = customSections.filter(s => s.name === 'name');
  if (nameSections.length) {
    // retrieve the raw custom section payloads via WebAssembly.Module.customSections
    try {
      const raw = WebAssembly.Module.customSections(module, 'name');
      if (raw && raw.length) {
        console.log('Decoded name section function names (top 7):');
        const functions = parseNameSectionFunctions(new Uint8Array(raw[0]));
        functions.slice(0, 7).forEach(f => console.log(`  idx ${f.idx}: ${f.name}`));
        if (functions.length > 7) console.log(`  ... and ${functions.length - 7} more`);
      }
    } catch (e) {
      console.error('Unable to decode name custom section', String(e));
    }
  }

  // heuristics
  const hasWasi = imports.some(i => i.module && i.module.toLowerCase().includes('wasi'));
  const hasWbg = imports.some(i => i.module && i.module.includes('__wbindgen')) || exports.some(e => e.name && e.name.startsWith('__wbindgen'));
  const hasEnv = imports.some(i => i.module === 'env');
  // Line up heuristics on one line for quick grep/CI parsing
  const heuristics = [
    `has_wasi=${hasWasi}`,
    `wasm_bindgen=${hasWbg}`,
    `has_env=${hasEnv}`,
    `_start=${exports.some(e => e.name === '_start')}`,
    `main=${exports.some(e => e.name === 'main')}`,
  ].join(', ');
  console.log('Heuristics:', heuristics);
}

/** Try to render the custom section content (first 128 bytes) as UTF-8 or hex snippet */
/**
 * Return a short UTF-8 or hex snippet of the raw payload
 * @param {Uint8Array|Buffer} raw
 * @returns {string}
 */
function snippetForCustomSection(raw) {
  const max = 128;
  const buf = raw instanceof Uint8Array ? raw : new Uint8Array(raw);
  const len = Math.min(max, buf.length);
  let s = '';
  try {
    const candidate = new TextDecoder().decode(buf.slice(0, len));
    // if the decode produces many control characters, return hex
    const control = candidate.split('').filter(c => (c.charCodeAt(0) < 0x09 || (c.charCodeAt(0) > 0x0d && c.charCodeAt(0) < 0x20))).length;
    if (control / Math.max(1, candidate.length) > 0.25) {
      throw new Error('non-text');
    }
    s = candidate;
  } catch (_) {
    // produce hex snippet
    const hex = Array.prototype.map.call(buf.slice(0, Math.min(len, 32)), b => b.toString(16).padStart(2, '0')).join(' ');
    s = `<hex ${hex}${buf.length > len ? ' ...' : ''}>`;
  }
  return s;
}

if (require.main === module) {
  run();
}
