/// <reference types="node" />
/// <reference path="./globals.d.ts" />

declare var fs: typeof import('fs');

declare var WASI: typeof import('wasi').WASI;

declare var wasi: import('wasi').WASI;

/** @type {Record<string, string>} */
declare var preopens;

// require
