(function() {
  const require = globalThis.__washm_bootstrap ? globalThis.__washm_bootstrap.require : null;
  const os = require ? require('os') : null;
  const process = globalThis.process || null;

  globalThis.mounted = globalThis.mounted || {};
  
  // Browser fallback values
  const hostname = os ? os.hostname() : (typeof window !== 'undefined' ? window.location.hostname : 'localhost');
  const platform = os ? os.platform() : (typeof navigator !== 'undefined' ? 'browser' : 'unknown');
  const arch = os ? os.arch() : (typeof navigator !== 'undefined' ? navigator.platform : 'unknown');
  const cpus = os ? os.cpus().length : (typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 1 : 1);
  const totalmem = os ? os.totalmem() : 0;
  const freemem = os ? os.freemem() : 0;
  const username = os ? os.userInfo().username : 'user';
  const uid = os ? os.userInfo().uid : 1000;
  const gid = os ? os.userInfo().gid : 1000;
  
  globalThis.mounted.washm = {
    startup: JSON.stringify({
      hostname,
      platform,
      arch,
      cpus,
      totalmem,
      freemem,
      userInfo: { username, uid, gid }
    }, null, 2),
    start_pwd: process ? (
      os.platform() === 'win32' ?
        process.cwd().replace(/^[a-zA-Z]:/, (m) => `/${m[0].toUpperCase()}:`).replace(/\\/g, '/') :
        process.cwd()
    ) : '/',
    exit_code: {
      _isVirtualFile: true,
      write(chunk) { this._code = (this._code || "") + chunk; },
      close() {
        const code = parseInt((this._code || "0").trim(), 10);
        if (process && process.exit) {
          process.exit(code);
        } else {
          console.log('Process exit with code:', code);
        }
      }
    }
  };
})();
