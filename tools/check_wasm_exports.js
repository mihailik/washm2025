#!/usr/bin/env node
// Simple wasm export checker for CI
// Usage: node tools/check_wasm_exports.js <path-to-wasm>

const fs = require('fs');
const wasmparser = require('wasmparser');

const argv = process.argv.slice(2);
if (argv.length < 1) {
  console.error('Usage: node tools/check_wasm_exports.js <path-to-wasm>');
  process.exit(2);
}

const wasmPath = argv[0];
const buffer = fs.readFileSync(wasmPath);

const allowed = new Set([
  '_continue',
  '_initialize',
  '_start',
  'memory',
  '__heap_base',
  '__data_end',
  '__stack_pointer'
]);

const reader = new wasmparser.BinaryReader();
reader.setData(buffer, 0, buffer.length);
let exports = [];
while (true) {
  const state = reader.read();
  if (state === wasmparser.BinaryReaderState.END) break;
  if (state === wasmparser.BinaryReaderState.BEGIN_SECTION) {
    const section = reader.result;
    if (section.id === wasmparser.SectionCode.Export) {
      const exportReader = reader.readExportSection();
      for (let i = 0; i < exportReader.getCount(); i++) {
        const exp = exportReader.read();
        exports.push(exp.field);
      }
    }
  }
}

const customExports = exports.filter(e => !allowed.has(e));
if (customExports.length) {
  console.error('Forbidden exports found in', wasmPath);
  customExports.forEach(e => console.error('  -', e));
  process.exit(3);
} else {
  console.log('OK: wasm exports are within allowed set:', Array.from(allowed).join(', '));
  process.exit(0);
}
