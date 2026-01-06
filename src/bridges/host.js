mounted['washm'] = createWashmMount();

function createWashmMount() {
  const os = isNode ? require('os') : undefined;

  let exitCode = undefined;
  
  const washmObj = {
    startup: JSON.stringify({
      hostname: isNode ? os.hostname() : typeof window.location.hostname || 'localhost',
      platform: isNode ? process.platform : getBrowserPlatform(),
      arch: isNode ? process.arch : getBrowserArch(),
      cpus: isNode ? os.cpus().length : navigator.hardwareConcurrency || 1,
      totalmem: isNode ? os.totalmem() : (navigator.deviceMemory || 4) * 1024 * 1024 * 1024,
      freemem: isNode ? os.freemem() : (navigator.deviceMemory || 4) * 1024 * 1024 * 1024 / 2,
      userInfo: {
        username: isNode ? os.userInfo().username : 'anonymous',
        uid: isNode ? os.userInfo().uid : 1000,
        gid: isNode ? os.userInfo().gid : 1000
      }
    }, null, 2),
    start_pwd: isNode ? (
      os.platform() === 'win32' ?
        process.cwd().replace(/^[a-zA-Z]:/, (m) => `/${m[0].toUpperCase()}:`).replace(/\\/g, '/') :
        process.cwd()
    ) :
      '/',
    get exit_code() {
      return exitCode;
    },
    set exit_code(code) {
      if (code instanceof Uint8Array)
        code = Number(new TextDecoder().decode(code).trim());

      if (isNode) process.exit(code);
      else console.log('Process exit with code:', code);
      exitCode = code;
    }
  };

  return washmObj;

  function getBrowserPlatform() {
    let platform = navigator.userAgentData?.platform;
    if (!platform) {
      const sources = [navigator.platform, navigator.userAgent];
      if (sources.some(s => /Mac/.test(s))) return 'macos';
      if (sources.some(s => /Win/.test(s))) return 'windows';
      if (sources.some(s => /Linux/.test(s))) return 'linux';
      platform = 'browser';
    }
    return platform;
  }

  function getBrowserArch() {
    let arch = navigator.userAgentData?.architecture;
    if (!arch) {
      const sources = [navigator.platform, navigator.userAgent];
      if (sources.some(s => /x86_64|Win64|WOW64|amd64|x64/.test(s))) return 'x64';
      if (sources.some(s => /i386|i686|x86/.test(s))) return 'x86';
      if (sources.some(s => /arm|aarch64/.test(s))) return 'arm';
      arch = 'unknown';
    }
    return arch;
  }
}