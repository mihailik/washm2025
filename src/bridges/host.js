(function() {
  const require = globalThis.__washm_bootstrap.require;
  const os = require('os');

  globalThis.mounted = globalThis.mounted || {};
  globalThis.mounted.washm = {
    startup: JSON.stringify({
      hostname: os.hostname(),
      platform: os.platform(),
      arch: os.arch(),
      cpus: os.cpus().length,
      totalmem: os.totalmem(),
      freemem: os.freemem(),
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
  };
})();
