(function() {
    // WASI Patching Mechanism
    const patchWasi = (name, impl) => {
        if (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.wasiSnapshot && globalThis.__washm_bootstrap.wasiSnapshot[name]) {
            const original = globalThis.__washm_bootstrap.wasiSnapshot[name].actual;
            globalThis.__washm_bootstrap.wasiSnapshot[name].actual = function() {
                const res = impl.apply(this, arguments);
                if (res === -1) {
                    return original.apply(this, arguments);
                }
                return res;
            };
        } else {
            console.warn(`Failed to patch WASI function: ${name}`);
        }
    };

    const FS_PREFIX = 'washm_fs_';
    const fsGet = (path) => sessionStorage.getItem(FS_PREFIX + path);
    const fsSet = (path, data) => sessionStorage.setItem(FS_PREFIX + path, data);
    const fsDel = (path) => sessionStorage.removeItem(FS_PREFIX + path);
    const fsList = () => {
        const files = [];
        for (let i = 0; i < sessionStorage.length; i++) {
            const key = sessionStorage.key(i);
            if (key && key.startsWith(FS_PREFIX)) {
                files.push(key.substring(FS_PREFIX.length));
            }
        }
        return files;
    };

    const openFiles = new Map();
    let nextFd = 10;
    const DIR_MARKER = '[[DIR]]';

    patchWasi('path_open', function(dirfd, dirflags, pathPtr, pathLen, oflags, rightsBase, rightsInheriting, fdflags, fdPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        
        if (normPath === '.' || normPath === '') {
             const fd = nextFd++;
             openFiles.set(fd, { path: '.', pos: 0, flags: oflags, isDir: true });
             new DataView(memory.buffer).setUint32(fdPtr, fd, true);
             return 0;
        }

        let content = fsGet(normPath);
        
        if (content === DIR_MARKER) {
             const fd = nextFd++;
             openFiles.set(fd, { path: normPath, pos: 0, flags: oflags, isDir: true });
             new DataView(memory.buffer).setUint32(fdPtr, fd, true);
             return 0;
        }

        if (content === null && (oflags & 1)) { // O_CREAT
            content = '';
            fsSet(normPath, content);
        } else if (content !== null && (oflags & 4)) { // O_TRUNC
            content = '';
            fsSet(normPath, content);
        }
        
        if (content !== null || (oflags & 1)) {
            const fd = nextFd++;
            openFiles.set(fd, { path: normPath, pos: 0, flags: oflags, isDir: false });
            new DataView(memory.buffer).setUint32(fdPtr, fd, true);
            return 0;
        }
        return 44; // ENOENT
    });

    patchWasi('fd_read', function(fd, iovsPtr, iovsLen, nreadPtr) {
        const file = openFiles.get(fd);
        if (!file) return -1; // Delegate
        if (file.isDir) return 28; // EINVAL
        
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const view = new DataView(memory.buffer);
        const content = fsGet(file.path) || '';
        const encoder = new TextEncoder();
        const bytes = encoder.encode(content);
        
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
    });

    patchWasi('fd_write', function(fd, iovsPtr, iovsLen, nwrittenPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const view = new DataView(memory.buffer);
        
        const file = openFiles.get(fd);
        if (!file) return -1; // Delegate
        if (file.isDir) return 28; // EINVAL
        
        let content = fsGet(file.path) || '';
        let totalWritten = 0;
        for (let i = 0; i < iovsLen; i++) {
            const bufPtr = view.getUint32(iovsPtr + i * 8, true);
            const bufLen = view.getUint32(iovsPtr + i * 8 + 4, true);
            const bytes = new Uint8Array(memory.buffer, bufPtr, bufLen);
            content += new TextDecoder().decode(bytes);
            totalWritten += bufLen;
        }
        fsSet(file.path, content);
        view.setUint32(nwrittenPtr, totalWritten, true);
        return 0;
    });

    patchWasi('fd_close', function(fd) {
        if (openFiles.has(fd)) {
            openFiles.delete(fd);
            return 0;
        }
        return -1; // Delegate
    });

    patchWasi('path_create_directory', function(dirfd, pathPtr, pathLen) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        if (fsGet(normPath) !== null) return 17; // EEXIST
        fsSet(normPath, DIR_MARKER);
        return 0;
    });

    patchWasi('path_unlink_file', function(dirfd, pathPtr, pathLen) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        fsDel(normPath);
        return 0;
    });

    patchWasi('fd_readdir', function(fd, bufPtr, bufLen, cookie, bufusedPtr) {
        const files = fsList();
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const encoder = new TextEncoder();
        
        const entries = [
            { name: '.', type: 3 },
            { name: '..', type: 3 },
            ...files.map(f => {
                const content = fsGet(f);
                return { name: f, type: content === DIR_MARKER ? 3 : 4 };
            })
        ];

        let offset = 0;
        for (let i = cookie; i < entries.length && offset < bufLen; i++) {
            const entry = entries[i];
            const nameBytes = encoder.encode(entry.name);
            if (offset + 24 + nameBytes.length > bufLen) break;
            
            const view = new DataView(memory.buffer);
            view.setBigUint64(bufPtr + offset, BigInt(i + 1), true); // d_next
            view.setBigUint64(bufPtr + offset + 8, BigInt(0), true); // d_ino
            view.setUint32(bufPtr + offset + 16, nameBytes.length, true); // d_namlen
            view.setUint8(bufPtr + offset + 20, entry.type); // d_type
            
            new Uint8Array(memory.buffer).set(nameBytes, bufPtr + offset + 24);
            offset += 24 + nameBytes.length;
        }
        new DataView(memory.buffer).setUint32(bufusedPtr, offset, true);
        return 0;
    });

    patchWasi('fd_filestat_get', function(fd, bufPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const view = new DataView(memory.buffer);
        
        if (fd === 3) { // Root preopen
             view.setUint8(bufPtr, 3); // FILETYPE_DIRECTORY
             view.setBigUint64(bufPtr + 8, BigInt(0), true);
             return 0;
        }

        const file = openFiles.get(fd);
        if (!file) return -1; // Delegate
        
        if (file.isDir) {
             view.setUint8(bufPtr, 3); // FILETYPE_DIRECTORY
             view.setBigUint64(bufPtr + 8, BigInt(0), true);
        } else {
             view.setUint8(bufPtr, 4); // FILETYPE_REGULAR_FILE
             const content = fsGet(file.path) || '';
             view.setBigUint64(bufPtr + 8, BigInt(content.length), true);
        }
        return 0;
    });

    patchWasi('path_filestat_get', function(dirfd, flags, pathPtr, pathLen, bufPtr) {
        const memory = globalThis.__washm_bootstrap.instance.exports.memory;
        const pathBytes = new Uint8Array(memory.buffer, pathPtr, pathLen);
        const path = new TextDecoder().decode(pathBytes);
        const normPath = path.startsWith('/') ? path.substring(1) : path;
        const view = new DataView(memory.buffer);

        if (normPath === '.' || normPath === '') {
             view.setUint8(bufPtr, 3); // FILETYPE_DIRECTORY
             view.setBigUint64(bufPtr + 8, BigInt(0), true);
             return 0;
        }

        const content = fsGet(normPath);
        if (content !== null) {
             if (content === DIR_MARKER) {
                 view.setUint8(bufPtr, 3); // FILETYPE_DIRECTORY
                 view.setBigUint64(bufPtr + 8, BigInt(0), true);
             } else {
                 view.setUint8(bufPtr, 4); // FILETYPE_REGULAR_FILE
                 view.setBigUint64(bufPtr + 8, BigInt(content.length), true);
             }
             return 0;
        }
        
        return 44; // ENOENT
    });

    return { ok: true };
})();