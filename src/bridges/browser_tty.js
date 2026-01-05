const terminalDiv = document.createElement('div');
terminalDiv.id = 'terminal';
terminalDiv.style.width = '100%';
terminalDiv.style.height = '100vh';
terminalDiv.style.margin = '0';
terminalDiv.style.padding = '0';
terminalDiv.style.overflow = 'hidden';
document.body.innerHTML = '';
document.body.appendChild(terminalDiv);
document.body.style.margin = '0';
document.body.style.padding = '0';
document.body.style.background = '#000';

const link = document.createElement('link');
link.rel = 'icon';
link.href = 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><text y=".9em" font-size="90">🐚</text></svg>';
document.head.appendChild(link);

document.title = 'WASHM';

const Terminal = window.Terminal;
const FitAddon = window.FitAddon ? window.FitAddon.FitAddon : (window.FitAddon || window.xtermAddonFit);

if (!Terminal) {
  throw new Error("Xterm.js not loaded correctly");
}

const term = new Terminal({
  cursorBlink: true,
  convertEol: true,
  fontFamily: 'Menlo, Monaco, "Courier New", monospace',
  fontSize: 14,
  theme: {
    background: '#000000',
    foreground: '#00ff00'
  },
  screenReaderMode: true
});

const fitAddon = FitAddon ? new FitAddon() : null;
if (fitAddon) {
  term.loadAddon(fitAddon);
}

term.open(terminalDiv);
if (fitAddon) fitAddon.fit();

window.addEventListener('resize', () => {
  if (fitAddon) fitAddon.fit();
});

term.focus();

let inputBuffer = [];
let pendingResolve = null;

term.onData(data => {
  if (pendingResolve) {
    const resolve = pendingResolve;
    pendingResolve = null;
    resolve(data);
  } else {
    inputBuffer.push(data);
  }
});

// WASI Patching Mechanism
const patchWasi = (name, impl) => {
  if (globalThis.__washm_bootstrap && globalThis.__washm_bootstrap.wasiSnapshot && globalThis.__washm_bootstrap.wasiSnapshot[name]) {
    const original = globalThis.__washm_bootstrap.wasiSnapshot[name].actual;
    globalThis.__washm_bootstrap.wasiSnapshot[name].actual = function () {
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



patchWasi('fd_write', function (fd, iovsPtr, iovsLen, nwrittenPtr) {
  const memory = globalThis.__washm_bootstrap.instance.exports.memory;
  const view = new DataView(memory.buffer);

  if (fd === 1 || fd === 2) {
    const uint8Array = new Uint8Array(memory.buffer);
    let total = 0;
    for (let i = 0; i < iovsLen; i++) {
      const ptr = view.getUint32(iovsPtr + i * 8, true);
      const len = view.getUint32(iovsPtr + i * 8 + 4, true);
      const chunk = uint8Array.slice(ptr, ptr + len);
      term.write(chunk);
      total += len;
    }
    view.setUint32(nwrittenPtr, total, true);
    return 0;
  }

  return -1; // Delegate
});



globalThis.washm_tty = {
  read: () => {
    return new Promise((resolve) => {
      if (inputBuffer.length > 0) {
        resolve(inputBuffer.shift());
      } else {
        pendingResolve = resolve;
      }
    });
  },
  enableRaw: () => { return { ok: true }; },
  disableRaw: () => { return { ok: true }; },
  size: () => {
    return {
      ok: true,
      rows: term.rows,
      cols: term.cols
    };
  }
};
