(function() {
  const require = globalThis.__washm_bootstrap.require;
  const os = require('os');

  globalThis.mounted = globalThis.mounted || {};
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
          cwd: (
            os.platform() === 'win32' && req.cwd ?
              (/^\/[a-zA-Z]:/.test(req.cwd) ? req.cwd.slice(1) : req.cwd).replace(/\//g, '\\') :
              req.cwd
          ) || process.cwd(),
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
