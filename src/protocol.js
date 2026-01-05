const target = globalThis.__washm_bootstrap.wasiSnapshot;
const memory = globalThis.__washm_bootstrap.instance.exports.memory;
const require = globalThis.__washm_bootstrap ? globalThis.__washm_bootstrap.require : null;

const fdToVfs = new Map();
let nextFakeFd = 20260105;
globalThis.mounted = {};

function readStringFromMemory(ptr, len) {
  return new TextDecoder().decode(new Uint8Array(memory.buffer, ptr, len));
}

function writeU32(ptr, val) {
  new DataView(memory.buffer).setUint32(ptr, val, true);
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
    if (targetVal) {
      const fd = nextFakeFd++;
      if (targetVal._isVirtualFile) {
        fdToVfs.set(fd, { target: targetVal, pos: 0, mode: 'rw', path: p });
      } else if (typeof targetVal === 'string') {
        fdToVfs.set(fd, { content: targetVal, pos: 0, mode: 'r', path: p });
      } else {
        fdToVfs.set(fd, { isDir: true, path: p });
      }
      writeU32(fd_out_ptr, fd);
      return 0;
    }
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
    let content = "";
    if (entry.target && entry.target.read) {
      content = entry.target.read();
    } else if (entry.content) {
      content = entry.content;
    }
    
    const buf = new TextEncoder().encode(content);
    const slice = buf.slice(entry.pos);
    const dv = new DataView(memory.buffer);
    let total = 0;
    let remaining = slice.length;
    for (let i = 0; i < iovs_len && remaining > 0; i++) {
      const off = iovs_ptr + i * 8;
      const bufPtr = dv.getUint32(off, true);
      const bufLen = dv.getUint32(off + 4, true);
      const toWrite = Math.min(bufLen, remaining);
      new Uint8Array(memory.buffer, bufPtr, toWrite).set(slice.slice(total, total + toWrite));
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
    const dv = new DataView(memory.buffer);
    let total = 0;
    for (let i = 0; i < iovs_len; i++) {
      const off = iovs_ptr + i * 8;
      const bufPtr = dv.getUint32(off, true);
      const bufLen = dv.getUint32(off + 4, true);
      const chunk = new TextDecoder().decode(new Uint8Array(memory.buffer, bufPtr, bufLen));
      if (entry.target && entry.target.write) entry.target.write(chunk);
      if (entry.write) entry.write(chunk);
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
    if (entry.target && entry.target.close) entry.target.close();
    if (entry.close) entry.close();
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
      const dv = new DataView(memory.buffer);
      for (let i = 0; i < 64; i++) dv.setUint8(buf_ptr + i, 0);
      const isDir = (targetVal && !targetVal._isDir && !targetVal._isVirtualFile && typeof targetVal !== 'string');
      dv.setUint8(buf_ptr + 16, isDir ? 3 : 4);
      let size = 0;
      if (typeof targetVal === 'string') {
        size = new TextEncoder().encode(targetVal).length;
      } else if (targetVal && targetVal.read) {
        size = new TextEncoder().encode(targetVal.read()).length;
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
