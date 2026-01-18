// @ts-check
/// <reference path="../globals.d.ts" />

initBrowserFS();

function initBrowserFS() {

  const openFiles = new Map();
  let nextFd = 10;

  wasiSnapshot.path_open.actual = browserWasi_path_open;
  wasiSnapshot.fd_read.actual = browserWasi_fd_read;
  const original_fd_write = wasiSnapshot.fd_write.actual;
  wasiSnapshot.fd_write.actual = browserWasi_fd_write;
  const original_fd_close = wasiSnapshot.fd_close.actual;
  wasiSnapshot.fd_close.actual = browserWasi_fd_close;
  wasiSnapshot.path_create_directory.actual = browserWasi_path_create_directory;
  wasiSnapshot.path_unlink_file.actual = browserWasi_path_unlink_file;
  wasiSnapshot.fd_readdir.actual = browserWasi_fd_readdir;
  wasiSnapshot.fd_filestat_get.actual = browserWasi_fd_filestat_get;
  wasiSnapshot.path_filestat_get.actual = browserWasi_path_filestat_get;

  function browserWasi_path_open(dirfd, dirflags, pathPtr, pathLen, oflags, rightsBase, rightsInheriting, fdflags, fdPtr) {
    const path = new TextDecoder().decode(new Uint8Array(instance.exports.memory.buffer, pathPtr, pathLen));
    const normPath = path.startsWith('/') ? path.substring(1) : path;

    if (normPath === '.' || normPath === '') {
      const fd = nextFd++;
      openFiles.set(fd, { path: '.', pos: 0, isDir: true });
      new DataView(instance.exports.memory.buffer).setUint32(fdPtr, fd, true);
      return 0;
    }

    // 3. sessionStorage handling
    let content = sessionStorage.getItem('washm_fs_' + normPath);
    if (content === '[[DIR]]') {
      const fd = nextFd++;
      openFiles.set(fd, { path: normPath, pos: 0, isDir: true });
      new DataView(instance.exports.memory.buffer).setUint32(fdPtr, fd, true);
      return 0;
    }

    if (content === null && (oflags & 1)) { // O_CREAT
      content = '';
      sessionStorage.setItem('washm_fs_' + normPath, content);
    } else if (content !== null && (oflags & 4)) { // O_TRUNC
      content = '';
      sessionStorage.setItem('washm_fs_' + normPath, content);
    }

    if (content !== null || (oflags & 1)) {
      const fd = nextFd++;
      openFiles.set(fd, { path: normPath, pos: 0, isDir: false });
      new DataView(instance.exports.memory.buffer).setUint32(fdPtr, fd, true);
      return 0;
    }
    return 44; // ENOENT
  }

  function browserWasi_fd_read(fd, iovsPtr, iovsLen, nreadPtr) {
    const file = openFiles.get(fd);
    if (!file) return 44;
    if (file.isDir) return 31; // EISDIR

    const view = new DataView(instance.exports.memory.buffer);

    let bytes;
    if (file.vfs !== undefined) {
      if (file.isDir) return 31; // EISDIR
      const val = file.vfs;
      if (val instanceof Uint8Array) {
        bytes = val;
      } else if (typeof val === 'string') {
        bytes = new TextEncoder().encode(val);
      } else if (val !== null && val !== undefined && typeof val !== 'object') {
        bytes = new TextEncoder().encode(String(val));
      } else {
        bytes = new Uint8Array(0);
      }
    } else {
      bytes = new TextEncoder().encode(sessionStorage.getItem('washm_fs_' + file.path) || '');
    }

    let totalRead = 0;
    for (let i = 0; i < iovsLen; i++) {
      const bufPtr = view.getUint32(iovsPtr + i * 8, true);
      const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
      const toRead = Math.min(bufLen, bytes.length - file.pos);
      if (toRead > 0) {
        new Uint8Array(instance.exports.memory.buffer).set(bytes.slice(file.pos, file.pos + toRead), bufPtr);
        file.pos += toRead;
        totalRead += toRead;
      }
    }
    view.setUint32(nreadPtr, totalRead, true);
    return 0;
  }

  function browserWasi_fd_write(fd, iovsPtr, iovsLen, nwrittenPtr) {
    if (fd === FD_BOOTRAP || fd === 1 || fd === 2)
      return original_fd_write(fd, iovsPtr, iovsLen, nwrittenPtr);

    const file = openFiles.get(fd);
    if (!file) return 44;
    if (file.isDir) return 31; // EISDIR

    const view = new DataView(instance.exports.memory.buffer);
    let totalWritten = 0;

    for (let i = 0; i < iovsLen; i++) {
      const bufPtr = view.getUint32(iovsPtr + i * 8, true);
      const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
      const chunk = new Uint8Array(instance.exports.memory.buffer, bufPtr, bufLen);
      const copy = new Uint8Array(bufLen);
      copy.set(chunk);
      file.buffers = file.buffers || [];
      file.buffers.push(copy);
      totalWritten += bufLen;
    }

    view.setUint32(nwrittenPtr, totalWritten, true);
    return 0;
  }

  function browserWasi_fd_close(fd) {
    if (fd === FD_BOOTRAP)
      return original_fd_close(fd);

    const file = openFiles.get(fd);
    if (!file) return 44;
    if (file.isDir) return openFiles.delete(fd) ? 0 : 44;

    const buffers = file.buffers || [];
    if (file.vfs !== undefined) {
      // persist into globalThis.mounted as Uint8Array
      const parts = file.path.split('/');
      let curr = globalThis.mounted;
      for (let i = 2; i < parts.length - 1; i++) {
        const p = parts[i];
        if (!curr[p]) curr[p] = {};
        if (typeof curr[p] !== 'object') {
          console.warn('mount path conflict, cannot create directory for', file.path);
          curr = null; break;
        }
        curr = curr[p];
      }
      if (curr) {
        const name = parts[parts.length - 1];
        const totalLen = buffers.reduce((s, b) => s + b.length, 0);
        const out = new Uint8Array(totalLen);
        let off = 0;
        for (const b of buffers) { out.set(b, off); off += b.length; }
        curr[name] = out;
      }
    } else {
      if (buffers.length > 0) {
        const totalLen = buffers.reduce((s, b) => s + b.length, 0);
        const out = new Uint8Array(totalLen);
        let off = 0;
        for (const b of buffers) { out.set(b, off); off += b.length; }
        let existing = sessionStorage.getItem('washm_fs_' + file.path) || '';
        existing += new TextDecoder().decode(out);
        sessionStorage.setItem('washm_fs_' + file.path, existing);
      }
    }

    openFiles.delete(fd);
    return 0;
  }

  function browserWasi_path_create_directory(dirfd, pathPtr, pathLen) {
    const path = new TextDecoder().decode(new Uint8Array(instance.exports.memory.buffer, pathPtr, pathLen));
    const normPath = path.startsWith('/') ? path.substring(1) : path;
    if (sessionStorage.getItem('washm_fs_' + normPath) !== null) return 17; // EEXIST
    sessionStorage.setItem('washm_fs_' + normPath, '[[DIR]]');
    return 0;
  }

  function browserWasi_path_unlink_file(dirfd, pathPtr, pathLen) {
    const path = new TextDecoder().decode(new Uint8Array(instance.exports.memory.buffer, pathPtr, pathLen));
    sessionStorage.removeItem('washm_fs_' + (path.startsWith('/') ? path.substring(1) : path));
    return 0;
  }

  function browserWasi_fd_readdir(fd, bufPtr, bufLen, cookie, bufusedPtr) {
    const file = openFiles.get(fd);
    if (!file || !file.isDir) return 44;

    const view = new DataView(instance.exports.memory.buffer);
    const entries = [{ name: '.', type: 3 }, { name: '..', type: 3 }];

    for (let i = 0; i < sessionStorage.length; i++) {
      const key = sessionStorage.key(i);
      if (key?.startsWith('washm_fs_')) {
        const name = key.substring(9);
        entries.push({ name, type: sessionStorage.getItem(key) === '[[DIR]]' ? 3 : 4 });
      }
    }

    const encoder = new TextEncoder();
    let offset = 0;
    for (let i = Number(cookie); i < entries.length && offset < bufLen; i++) {
      const entry = entries[i];
      const nameBytes = encoder.encode(entry.name);
      if (offset + 24 + nameBytes.length > bufLen) break;
      view.setBigUint64(bufPtr + offset, BigInt(i + 1), true);
      view.setBigUint64(bufPtr + offset + 8, 0n, true);
      view.setUint32(bufPtr + offset + 16, nameBytes.length, true);
      view.setUint8(bufPtr + offset + 20, entry.type);
      new Uint8Array(instance.exports.memory.buffer).set(nameBytes, bufPtr + offset + 24);
      offset += 24 + nameBytes.length;
    }
    view.setUint32(bufusedPtr, offset, true);
    return 0;
  }

  function browserWasi_fd_filestat_get(fd, bufPtr) {
    const file = openFiles.get(fd);
    if (!file) return 44;

    const view = new DataView(instance.exports.memory.buffer);

    let size = 0;
    if (file.vfs !== undefined) {
      const val = file.vfs;
      if (val instanceof Uint8Array) size = val.length;
      else if (typeof val === 'string') size = new TextEncoder().encode(val).length;
      else if (val !== null && val !== undefined && typeof val !== 'object') size = new TextEncoder().encode(String(val)).length;
      else size = 0;
    } else {
      const content = sessionStorage.getItem('washm_fs_' + file.path) || '';
      size = content === '[[DIR]]' ? 0 : new TextEncoder().encode(content).length;
    }

    view.setUint8(bufPtr + 16, file.isDir ? 3 : 4);
    view.setBigUint64(bufPtr + 32, BigInt(size), true);
    return 0;
  }

  function browserWasi_path_filestat_get(dirfd, flags, pathPtr, pathLen, bufPtr) {
    const path = new TextDecoder().decode(new Uint8Array(instance.exports.memory.buffer, pathPtr, pathLen));
    const normPath = path.startsWith('/') ? path.substring(1) : path;
    const view = new DataView(instance.exports.memory.buffer);

    if (!normPath || normPath === '.') {
      view.setUint8(bufPtr + 16, 3);
      view.setBigUint64(bufPtr + 32, 0n, true);
      return 0;
    }

    const content = sessionStorage.getItem('washm_fs_' + normPath);
    if (content === null) return 44;
    const isDir = content === '[[DIR]]';
    view.setUint8(bufPtr + 16, isDir ? 3 : 4);
    view.setBigUint64(bufPtr + 32, isDir ? 0n : BigInt(new TextEncoder().encode(content).length), true);
    return 0;
  }
}