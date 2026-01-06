// @ts-check
/// <reference path="./globals.d.ts" />

const target = globalThis.__washm_bootstrap.wasiSnapshot;

const fdToVfs = new Map();
let nextFakeFd = 20260105;
globalThis.mounted = {};

function readStringFromMemory(ptr, len) {
  return new TextDecoder().decode(new Uint8Array(instance.exports.memory.buffer, ptr, len));
}

function writeU32(ptr, val) {
  new DataView(instance.exports.memory.buffer).setUint32(ptr, val, true);
}

function normalizePath(p) {
  if (p.startsWith('/')) p = p.substring(1);
  return p;
}

function resolveMounted(path) {
  const parts = path.split('/');
  if (parts[0] === 'dev' && parts[1] === 'host') {
    let curr = globalThis.mounted;
    for (let i = 2; i < parts.length; i++) {
      if (!curr || typeof curr !== 'object') return null;
      curr = curr[parts[i]];
    }
    return curr;
  }
  return null;
}

const orig_path_open = target.path_open ? target.path_open.actual : null;
target.path_open.actual = function (dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr) {
  let p = readStringFromMemory(path_ptr, path_len);
  p = normalizePath(p);
  if (p.startsWith('dev/host/') || p === 'dev/host') {
    const targetVal = resolveMounted(p);
    const fd = nextFakeFd++;
    // determine parent and name parts for potential creation on close
    const parts = p.split('/');
    const name = parts[parts.length - 1];
    const parentPath = parts.slice(0, -1).join('/');
    const parent = parentPath ? resolveMounted(parentPath) : globalThis.mounted;

    if (targetVal && typeof targetVal === 'object' && !(targetVal instanceof Uint8Array)) {
      // directory
      fdToVfs.set(fd, { isDir: true, path: p });
    } else {
      // file (existing or new)
      let contentBytes = null;
      if (targetVal instanceof Uint8Array) {
        contentBytes = targetVal;
      } else if (typeof targetVal === 'string') {
        contentBytes = new TextEncoder().encode(targetVal);
      } else if (targetVal !== undefined && typeof targetVal !== 'object') {
        contentBytes = new TextEncoder().encode(String(targetVal));
      }
      fdToVfs.set(fd, { path: p, pos: 0, content: contentBytes, buffers: [], parent, name });
    }
    writeU32(fd_out_ptr, fd);
    return 0;
  }
  if (orig_path_open) {
    return orig_path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  }
  return 28; // ENOSYS
};

const orig_fd_read = target.fd_read ? target.fd_read.actual : null;
target.fd_read.actual = function (fd, iovs_ptr, iovs_len, nread_ptr) {
  if (fdToVfs.has(fd)) {
    const entry = fdToVfs.get(fd);
    if (entry.isDir) return 31; // EISDIR

    // Resolve the current stored value (may have changed since open)
    const targetVal = resolveMounted(entry.path);
    let buf;
    if (targetVal instanceof Uint8Array) {
      buf = targetVal;
    } else if (typeof targetVal === 'string') {
      buf = new TextEncoder().encode(targetVal);
    } else if (targetVal !== undefined && targetVal !== null && typeof targetVal !== 'object') {
      buf = new TextEncoder().encode(String(targetVal));
    } else {
      // no stored content; fall back to any content captured at open
      buf = entry.content || new Uint8Array(0);
    }

    const slice = buf.slice(entry.pos);
    const dv = new DataView(instance.exports.memory.buffer);
    let total = 0;
    let remaining = slice.length;
    for (let i = 0; i < iovs_len && remaining > 0; i++) {
      const off = iovs_ptr + i * 8;
      const bufPtr = dv.getUint32(off, true);
      const bufLen = dv.getUint32(off + 4, true);
      const toWrite = Math.min(bufLen, remaining);
      new Uint8Array(instance.exports.memory.buffer, bufPtr, toWrite).set(slice.slice(total, total + toWrite));
      total += toWrite;
      remaining -= toWrite;
    }
    entry.pos += total;
    writeU32(nread_ptr, total);
    return 0;
  }
  if (orig_fd_read) {
    return orig_fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  }
  return 28; // ENOSYS
};

const orig_fd_write = target.fd_write ? target.fd_write.actual : null;
target.fd_write.actual = function (fd, iovs_ptr, iovs_len, nwritten_ptr) {
  if (fdToVfs.has(fd)) {
    const entry = fdToVfs.get(fd);
    if (entry.isDir) return 31; // EISDIR
    const dv = new DataView(instance.exports.memory.buffer);
    let total = 0;
    for (let i = 0; i < iovs_len; i++) {
      const off = iovs_ptr + i * 8;
      const bufPtr = dv.getUint32(off, true);
      const bufLen = dv.getUint32(off + 4, true);
      const chunk = new Uint8Array(instance.exports.memory.buffer, bufPtr, bufLen);
      // make a copy of the chunk
      const copy = new Uint8Array(bufLen);
      copy.set(chunk);
      entry.buffers = entry.buffers || [];
      entry.buffers.push(copy);
      total += bufLen;
    }
    writeU32(nwritten_ptr, total);
    return 0;
  }
  if (orig_fd_write) {
    return orig_fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr);
  }
  return 28; // ENOSYS
};

const orig_fd_close = target.fd_close ? target.fd_close.actual : null;
target.fd_close.actual = function (fd) {
  if (fdToVfs.has(fd)) {
    const entry = fdToVfs.get(fd);
    if (entry.isDir) {
      fdToVfs.delete(fd);
      return 0;
    }
    if (entry.buffers && entry.buffers.length > 0) {
      const totalLen = entry.buffers.reduce((s, b) => s + b.length, 0);
      const out = new Uint8Array(totalLen);
      let off = 0;
      for (const b of entry.buffers) { out.set(b, off); off += b.length; }

      // assign into globalThis.mounted, creating parent directories when missing
      const parts = entry.path.split('/');
      let curr = globalThis.mounted;
      for (let i = 2; i < parts.length - 1; i++) {
        const p = parts[i];
        if (!curr[p]) curr[p] = {};
        if (typeof curr[p] !== 'object') {
          console.warn('mount path conflict, cannot create directory for', entry.path);
          curr = null; break;
        }
        curr = curr[p];
      }
      if (curr) {
        const name = parts[parts.length - 1];
        curr[name] = out;
      }
    }
    fdToVfs.delete(fd);
    return 0;
  }
  if (orig_fd_close) {
    return orig_fd_close(fd);
  }
  return 0; // Success
};

const orig_path_filestat_get = target.path_filestat_get ? target.path_filestat_get.actual : null;
target.path_filestat_get.actual = function (fd, flags, path_ptr, path_len, buf_ptr) {
  let p = readStringFromMemory(path_ptr, path_len);
  p = normalizePath(p);
  if (p.startsWith('dev/host/') || p === 'dev/host') {
    const targetVal = resolveMounted(p);
    if (targetVal || p === 'dev/host/washm/exit_code') {
      const dv = new DataView(instance.exports.memory.buffer);
      for (let i = 0; i < 64; i++) dv.setUint8(buf_ptr + i, 0);
      const isDir = (targetVal && typeof targetVal === 'object' && !(targetVal instanceof Uint8Array));
      dv.setUint8(buf_ptr + 16, isDir ? 3 : 4);
      let size = 0;
      if (!isDir && targetVal !== undefined && targetVal !== null) {
        if (targetVal instanceof Uint8Array) size = targetVal.length;
        else if (typeof targetVal === 'string') size = new TextEncoder().encode(targetVal).length;
        else size = new TextEncoder().encode(String(targetVal)).length;
      }
      dv.setBigUint64(buf_ptr + 32, BigInt(size), true);
      return 0;
    }
  }
  if (orig_path_filestat_get) {
    return orig_path_filestat_get(fd, flags, path_ptr, path_len, buf_ptr);
  }
  return 28; // ENOSYS
};
