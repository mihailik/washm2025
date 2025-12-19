(function() {
  const target = globalThis.__washm_bootstrap.wasiSnapshot;
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const require = globalThis.__washm_bootstrap.require;
  const os = require('os');

  const fdToVfs = new Map();
  let nextFakeFd = 500;

  const evalState = new Map();

  globalThis.mounted = {
    washm: {
      startup: JSON.stringify({
        hostname: os.hostname(),
        platform: os.platform(),
        arch: os.arch(),
        cpus: os.cpus().length,
        totalmem: os.totalmem(),
        freemem: os.freemem(),
        env: process.env,
        userInfo: {
          username: os.userInfo().username,
          uid: os.userInfo().uid,
          gid: os.userInfo().gid
        }
      }, null, 2),
      start_pwd: process.cwd(),
      exit_code: {
        _isVirtualFile: true,
        write(chunk) { this._code = (this._code || "") + chunk; },
        close() {
          const code = parseInt((this._code || "0").trim(), 10);
          process.exit(code);
        }
      }
    },
    eval: new Proxy({}, {
      get(target, prop) {
        if (typeof prop === 'string') {
          const parts = prop.split('.');
          const cookie = parts[0];
          const ext = parts[1];
          if (!evalState.has(cookie)) {
            evalState.set(cookie, { js: '', json: null });
          }
          const state = evalState.get(cookie);
          if (ext === 'js') {
            return {
              _isVirtualFile: true,
              path: `dev/host/eval/${prop}`,
              write: (chunk) => { state.js += chunk; },
              close: () => {
                let jsText = state.js;
                const handleResult = (result) => {
                  state.json = JSON.stringify({ ok: true, result });
                  const instance = globalThis.__washm_bootstrap.instance;
                  if (instance && typeof instance.exports._continue === 'function') {
                    instance.exports._continue(Number(cookie) || 0);
                  }
                };
                const handleError = (err) => {
                  console.error("VFS: JS Error:", err);
                  state.json = JSON.stringify({ ok: false, error: String(err) });
                  const instance = globalThis.__washm_bootstrap.instance;
                  if (instance && typeof instance.exports._continue === 'function') {
                    instance.exports._continue(Number(cookie) || 0);
                  }
                };
                try {
                  let result = eval(jsText);
                  if (result instanceof Promise) {
                    result.then(handleResult).catch(handleError);
                  } else {
                    handleResult(result);
                  }
                } catch (err) { handleError(err); }
              }
            };
          } else if (ext === 'json') {
            return {
              _isVirtualFile: true,
              path: `dev/host/eval/${prop}`,
              read: () => state.json || ''
            };
          }
        }
        return Reflect.get(target, prop);
      }
    })
  };

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

  const orig_path_open = target.path_open.actual;
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
    return orig_path_open(dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base, fs_rights_inheriting, fdflags, fd_out_ptr);
  };

  const orig_fd_read = target.fd_read.actual;
  target.fd_read.actual = function (fd, iovs_ptr, iovs_len, nread_ptr) {
    if (fdToVfs.has(fd)) {
      const entry = fdToVfs.get(fd);
      let content = "";
      if (entry.target && entry.target.read) {
        content = entry.target.read();
      } else if (entry.content) {
        content = entry.content;
      }
      
      const buf = Buffer.from(content);
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
    return orig_fd_read(fd, iovs_ptr, iovs_len, nread_ptr);
  };

  const orig_fd_write = target.fd_write.actual;
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
    return orig_fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr);
  };

  const orig_fd_close = target.fd_close.actual;
  target.fd_close.actual = function (fd) {
    if (fdToVfs.has(fd)) {
      const entry = fdToVfs.get(fd);
      if (entry.target && entry.target.close) entry.target.close();
      if (entry.close) entry.close();
      fdToVfs.delete(fd);
      return 0;
    }
    return orig_fd_close(fd);
  };

  const orig_path_filestat_get = target.path_filestat_get.actual;
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
          size = Buffer.from(targetVal).length;
        } else if (targetVal && targetVal.read) {
          size = Buffer.from(targetVal.read()).length;
        }
        dv.setBigUint64(buf_ptr + 32, BigInt(size), true);
        return 0;
      }
    }
    return orig_path_filestat_get(fd, flags, path_ptr, path_len, buf_ptr);
  };

  // Install process bridge
  globalThis.mounted.process = globalThis.mounted.process || {};
  globalThis.mounted.process['spawn.json'] = {
    _isVirtualFile: true,
    write(chunk) { this._buf = (this._buf || '') + chunk; },
    close() {
      try {
        if (!this._buf) return;
        const req = JSON.parse(this._buf);
        if (!req.program) return;
        const cp = require('child_process');
        const stdio = [
          req.fds && req.fds['0'] === 'pipe' ? 'pipe' : 'inherit',
          req.fds && req.fds['1'] === 'pipe' ? 'pipe' : 'inherit',
          req.fds && req.fds['2'] === 'pipe' ? 'pipe' : 'inherit'
        ];
        let programToSpawn = req.program || '';
        if (programToSpawn.includes('/') && !require('fs').existsSync(programToSpawn)) {
          programToSpawn = programToSpawn.split('/').pop();
        }
        const child = cp.spawn(programToSpawn, req.args || [], { 
          cwd: req.cwd || process.cwd(), 
          env: req.env ? Object.assign({}, process.env, req.env) : process.env, 
          stdio, 
          shell: true 
        });
        const pid = String(child.pid || -1);
        globalThis.mounted.process[pid] = { 
          _isDir: true, 
          stdout: { _isVirtualFile: true, read() { return this._buf || ''; } }, 
          stderr: { _isVirtualFile: true, read() { return this._buf || ''; } }, 
          status: { _isVirtualFile: true, read() { return JSON.stringify(this._status || {}); } }, 
          stdin: { _isVirtualFile: true, write(chunk) { if (child.stdin) child.stdin.write(chunk); }, close() { if (child.stdin) child.stdin.end(); } }, 
          signal: { _isVirtualFile: true, write(chunk) { try { child.kill(chunk.trim()); } catch(e) {} } } 
        };
        if (child.stdout) child.stdout.on('data', d => { globalThis.mounted.process[pid].stdout._buf = (globalThis.mounted.process[pid].stdout._buf || '') + d.toString(); });
        if (child.stderr) child.stderr.on('data', d => { globalThis.mounted.process[pid].stderr._buf = (globalThis.mounted.process[pid].stderr._buf || '') + d.toString(); });
        child.on('exit', (code, sig) => { 
          globalThis.mounted.process[pid].status._status = { exited: true, code: code !== null ? code : -1, signal: sig || null }; 
          const instance = globalThis.__washm_bootstrap.instance;
          if (instance && typeof instance.exports._continue === 'function') {
            instance.exports._continue(0); 
          }
        });
        this._reply = pid;
        this._buf = '';
      } catch (e) { 
        console.error("VFS: spawn error:", e);
        this._reply = JSON.stringify({ error: String(e) }); 
        this._buf = ''; 
      }
    },
    read() { return this._reply || ''; }
  };
})();
