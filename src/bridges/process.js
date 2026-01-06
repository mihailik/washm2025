// @ts-check
/// <reference path="../globals.node.d.ts" />

if (isNode) installNodeProcessBridge();

function installNodeProcessBridge() {
  const os = require('os');
  const child_process = require('child_process');

  const processTarget = {};
  globalThis.mounted.process = new Proxy(processTarget, {
    set(target, prop, value) {
      if (prop === 'spawn.json' && value instanceof Uint8Array) {
        try {
          const bufText = new TextDecoder().decode(value);
          const req = JSON.parse(bufText);
          if (!req.program) return true;
          const stdio = /** @type {const} */([
            req.fds && req.fds['0'] === 'pipe' ? 'pipe' : 'inherit',
            req.fds && req.fds['1'] === 'pipe' ? 'pipe' : 'inherit',
            req.fds && req.fds['2'] === 'pipe' ? 'pipe' : 'inherit'
          ]);

          let programToSpawn = req.program || '';
          if (programToSpawn.includes('/') && fs && !fs.existsSync(programToSpawn))
            programToSpawn = programToSpawn.split('/').pop();

          /** @type {import('child_process').ChildProcess} */
          const child = child_process.spawn(programToSpawn, req.args || [], {
            cwd: (
              os && os.platform() === 'win32' && req.cwd ?
                (/^\/[a-zA-Z]:/.test(req.cwd) ? req.cwd.slice(1) : req.cwd).replace(/\//g, '\\') :
                req.cwd
            ) || (process ? process.cwd() : '/'),
            env: req.env && process ? Object.assign({}, process.env, req.env) : (process ? process.env : {}),
            stdio,
            shell: true
          });
          const pid = String(child.pid || -1);

          // create child container with proxied set trap for stdin/signal
          const childState = { stdout: '', stderr: '', status: '' };
          const proxiedChild = new Proxy(childState, {
            set(t, key, val) {
              if (key === 'stdin' && val instanceof Uint8Array) {
                if (child.stdin) child.stdin.write(val);
                return true;
              }
              if (key === 'signal' && val instanceof Uint8Array) {
                try { child.kill(Number(new TextDecoder().decode(val).trim())); } catch (e) { }
                return true;
              }
              t[key] = val;
              return true;
            },
            get(t, key) { return t[key]; }
          });

          target[pid] = proxiedChild;

          if (child.stdout) child.stdout.on('data', d => { childState.stdout = (childState.stdout || '') + d.toString(); });
          if (child.stderr) child.stderr.on('data', d => { childState.stderr = (childState.stderr || '') + d.toString(); });
          child.on('exit', (code, sig) => {
            childState.status = JSON.stringify({ exited: true, code: code !== null ? code : -1, signal: sig || null });
            const instance = globalThis.__washm_bootstrap.instance;
            if (instance && typeof instance.exports._continue === 'function') {
              instance.exports._continue(0);
            }
          });

          // set the spawn.json property to the pid string so readers can obtain it
          target['spawn.json'] = pid;
        } catch (e) {
          console.error('VFS: spawn error:', e);
          target['spawn.json'] = JSON.stringify({ error: String(e) });
        }
        return true;
      }
      target[prop] = value;
      return true;
    },
    get(target, prop) { return target[prop]; }
  });
}