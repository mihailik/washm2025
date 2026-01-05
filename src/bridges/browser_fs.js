// Install WASI overrides by patching wasiSnapshot directly (no helper)

const FS_PREFIX = 'washm_fs_';
const openFiles = new Map();
let nextFd = 10;
const DIR_MARKER = '[[DIR]]';

const snapshot = globalThis.__washm_bootstrap.wasiSnapshot;

snapshot.path_open.actual = (dirfd, dirflags, pathPtr, pathLen, oflags, rightsBase, rightsInheriting, fdflags, fdPtr) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const path = new TextDecoder().decode(new Uint8Array(memory.buffer, pathPtr, pathLen));
  const normPath = path.startsWith('/') ? path.substring(1) : path;

  if (normPath === '.' || normPath === '') {
    const fd = nextFd++;
    openFiles.set(fd, { path: '.', pos: 0, flags: oflags, isDir: true });
    new DataView(memory.buffer).setUint32(fdPtr, fd, true);
    return 0;
  }

  let content = sessionStorage.getItem(FS_PREFIX + normPath);
  if (content === DIR_MARKER) {
    const fd = nextFd++;
    openFiles.set(fd, { path: normPath, pos: 0, flags: oflags, isDir: true });
    new DataView(memory.buffer).setUint32(fdPtr, fd, true);
    return 0;
  }

  if (content === null && (oflags & 1)) { // O_CREAT
    content = '';
    sessionStorage.setItem(FS_PREFIX + normPath, content);
  } else if (content !== null && (oflags & 4)) { // O_TRUNC
    content = '';
    sessionStorage.setItem(FS_PREFIX + normPath, content);
  }

  if (content !== null || (oflags & 1)) {
    const fd = nextFd++;
    openFiles.set(fd, { path: normPath, pos: 0, flags: oflags, isDir: false });
    new DataView(memory.buffer).setUint32(fdPtr, fd, true);
    return 0;
  }
  return 44; // ENOENT
};

snapshot.fd_read.actual = (fd, iovsPtr, iovsLen, nreadPtr) => {
  const file = openFiles.get(fd);
  if (!file) return 44;
  if (file.isDir) return 28;

  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const view = new DataView(memory.buffer);
  const bytes = new TextEncoder().encode(sessionStorage.getItem(FS_PREFIX + file.path) || '');

  let totalRead = 0;
  for (let i = 0; i < iovsLen; i++) {
    const bufPtr = view.getUint32(iovsPtr + i * 8, true);
    const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
    const toRead = Math.min(bufLen, bytes.length - file.pos);
    if (toRead > 0) {
      new Uint8Array(memory.buffer).set(bytes.slice(file.pos, file.pos + toRead), bufPtr);
      file.pos += toRead;
      totalRead += toRead;
    }
  }
  view.setUint32(nreadPtr, totalRead, true);
  return 0;
};

snapshot.fd_write.actual = (fd, iovsPtr, iovsLen, nwrittenPtr) => {
  const file = openFiles.get(fd);
  if (!file) return 44;
  if (file.isDir) return 28;

  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const view = new DataView(memory.buffer);
  let content = sessionStorage.getItem(FS_PREFIX + file.path) || '';
  let totalWritten = 0;
  for (let i = 0; i < iovsLen; i++) {
    const bufPtr = view.getUint32(iovsPtr + i * 8, true);
    const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
    content += new TextDecoder().decode(new Uint8Array(memory.buffer, bufPtr, bufLen));
    totalWritten += bufLen;
  }
  sessionStorage.setItem(FS_PREFIX + file.path, content);
  view.setUint32(nwrittenPtr, totalWritten, true);
  return 0;
};

snapshot.fd_close.actual = (fd) => openFiles.delete(fd) ? 0 : 44;

snapshot.path_create_directory.actual = (dirfd, pathPtr, pathLen) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const path = new TextDecoder().decode(new Uint8Array(memory.buffer, pathPtr, pathLen));
  const normPath = path.startsWith('/') ? path.substring(1) : path;
  if (sessionStorage.getItem(FS_PREFIX + normPath) !== null) return 17; // EEXIST
  sessionStorage.setItem(FS_PREFIX + normPath, DIR_MARKER);
  return 0;
};

snapshot.path_unlink_file.actual = (dirfd, pathPtr, pathLen) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const path = new TextDecoder().decode(new Uint8Array(memory.buffer, pathPtr, pathLen));
  sessionStorage.removeItem(FS_PREFIX + (path.startsWith('/') ? path.substring(1) : path));
  return 0;
};

snapshot.fd_readdir.actual = (fd, bufPtr, bufLen, cookie, bufusedPtr) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const view = new DataView(memory.buffer);
  const files = [];
  for (let i = 0; i < sessionStorage.length; i++) {
    const key = sessionStorage.key(i);
    if (key?.startsWith(FS_PREFIX)) files.push(key.substring(FS_PREFIX.length));
  }

  const entries = [
    { name: '.', type: 3 },
    { name: '..', type: 3 },
    ...files.map(f => ({ name: f, type: sessionStorage.getItem(FS_PREFIX + f) === DIR_MARKER ? 3 : 4 }))
  ];

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
    new Uint8Array(memory.buffer).set(nameBytes, bufPtr + offset + 24);
    offset += 24 + nameBytes.length;
  }
  view.setUint32(bufusedPtr, offset, true);
  return 0;
};

snapshot.fd_filestat_get.actual = (fd, bufPtr) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const view = new DataView(memory.buffer);
  if (fd === 3) {
    view.setUint8(bufPtr, 3);
    view.setBigUint64(bufPtr + 8, 0n, true);
    return 0;
  }
  const file = openFiles.get(fd);
  if (!file) return 44;
  const content = sessionStorage.getItem(FS_PREFIX + file.path) || '';
  view.setUint8(bufPtr, file.isDir ? 3 : 4);
  view.setBigUint64(bufPtr + 8, file.isDir ? 0n : BigInt(content.length), true);
  return 0;
};

snapshot.path_filestat_get.actual = (dirfd, flags, pathPtr, pathLen, bufPtr) => {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const path = new TextDecoder().decode(new Uint8Array(memory.buffer, pathPtr, pathLen));
  const normPath = path.startsWith('/') ? path.substring(1) : path;
  const view = new DataView(memory.buffer);
  if (!normPath || normPath === '.') {
    view.setUint8(bufPtr, 3);
    view.setBigUint64(bufPtr + 8, 0n, true);
    return 0;
  }
  const content = sessionStorage.getItem(FS_PREFIX + normPath);
  if (content === null) return 44;
  view.setUint8(bufPtr, content === DIR_MARKER ? 3 : 4);
  view.setBigUint64(bufPtr + 8, content === DIR_MARKER ? 0n : BigInt(content.length), true);
  return 0;
};
