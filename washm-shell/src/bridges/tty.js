// @ts-check
/// <reference path="../globals.node.d.ts" />

initNodeTty();

function initNodeTty() {
  /** @type {(Buffer | null)[]} */
  let buffer = [];
  /** @type {((data: Buffer | null) => void) | undefined} */
  let pendingResolve;
  let isRaw = false;

  if (process.stdin.isTTY) {
    process.stdin.setRawMode(true);
    isRaw = true;
  }

  process.stdin.setEncoding('utf8');
  process.stdin.on('data', handleStdinData);
  process.stdin.on('end', handleStdinEnd);
  process.stdin.resume();

  globalThis.washm_tty = {
    read,
    enableRaw,
    disableRaw,
    size
  };

  function read() {
    return new Promise((resolve) => {
      if (buffer.length > 0) {
        resolve(buffer.shift());
      } else {
        pendingResolve = resolve;
      }
    });
  }

  function enableRaw() {
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(true);
      isRaw = true;
      return /** @type {const} */({ ok: true });
    }
    return /** @type {const} */({ ok: false, error: "Not a TTY" });
  }

  function disableRaw() {
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(false);
      isRaw = false;
      return /** @type {const} */({ ok: true });
    }
    return /** @type {const} */({ ok: false, error: "Not a TTY" });
  }

  function size() {
    if (process.stdout.isTTY) {
      return /** @type {const} */({ ok: true, rows: process.stdout.rows, cols: process.stdout.columns });
    }
    return /** @type {const} */({ ok: false, error: "Not a TTY" });
  }

  /** @param {Buffer} chunk */
  function handleStdinData(chunk) {
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = undefined;
      resolve(chunk);
    } else {
      buffer.push(chunk);
    }
  }

  function handleStdinEnd() {
    if (pendingResolve) {
      const resolve = pendingResolve;
      pendingResolve = undefined;
      resolve(null); // EOF
    } else {
      buffer.push(null);
    }
  }
}