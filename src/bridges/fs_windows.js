(function() {
  if (typeof process === 'undefined' || process.platform !== 'win32') {
    return;
  }

  const target = globalThis.__washm_bootstrap.wasiSnapshot;
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;

  const orig_fd_prestat_get = target.fd_prestat_get.actual;
  const orig_fd_prestat_dir_name = target.fd_prestat_dir_name.actual;
  const orig_path_open = target.path_open.actual;
  const orig_fd_readdir = target.fd_readdir.actual;
  const orig_fd_fdstat_get = target.fd_fdstat_get.actual;

  let syntheticRootFd = -1;

  function writeU32(ptr, val) {
    new DataView(memory.buffer).setUint32(ptr, val, true);
  }

  target.fd_prestat_get.actual = function(fd, bufPtr) {
    const ret = orig_fd_prestat_get(fd, bufPtr);
    if (ret === 0) return 0;

    // If the original call failed, check if this is the next available FD
    // and if we haven't assigned a synthetic root yet.
    // We assume sequential allocation of preopens.
    if (syntheticRootFd === -1) {
        // We claim this FD.
        syntheticRootFd = fd;
    }

    if (fd === syntheticRootFd) {
        const dv = new DataView(memory.buffer);
        dv.setUint8(bufPtr, 0); // tag: dir (0)
        dv.setUint32(bufPtr + 4, 1, true); // length of "/" is 1
        return 0;
    }

    return ret;
  };

  target.fd_prestat_dir_name.actual = function(fd, pathPtr, pathLen) {
    if (fd === syntheticRootFd) {
        if (pathLen < 1) return 28; // EINVAL
        const arr = new Uint8Array(memory.buffer, pathPtr, 1);
        arr[0] = 47; // '/'
        return 0;
    }
    return orig_fd_prestat_dir_name(fd, pathPtr, pathLen);
  };

  target.fd_fdstat_get.actual = function(fd, bufPtr) {
    if (fd === syntheticRootFd) {
        const dv = new DataView(memory.buffer);
        dv.setUint8(bufPtr, 3); // fs_filetype: directory (3)
        dv.setUint16(bufPtr + 2, 0, true); // fs_flags
        // Rights: give full rights
        dv.setBigUint64(bufPtr + 8, 0xFFFFFFFFFFFFFFFFn, true);
        dv.setBigUint64(bufPtr + 16, 0xFFFFFFFFFFFFFFFFn, true);
        return 0;
    }
    return orig_fd_fdstat_get(fd, bufPtr);
  };

  target.path_open.actual = function(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr) {
    if (dirfd === syntheticRootFd) {
        // If we reached here, it means protocol.js (if loaded later) didn't handle it.
        // So it's not /dev/host.
        // We return ENOENT for now.
        return 44; // ENOENT
    }
    return orig_path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };

  target.fd_readdir.actual = function(fd, buf_ptr, buf_len, cookie, bufused_ptr) {
    if (fd === syntheticRootFd) {
        writeU32(bufused_ptr, 0);
        return 0;
    }
    return orig_fd_readdir(fd, buf_ptr, buf_len, cookie, bufused_ptr);
  };

})();
