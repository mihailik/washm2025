// @ts-check

const evalState = new Map();

globalThis.mounted.eval = new Proxy({}, {
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
            try {
              let result = eval(jsText);
              if (result instanceof Promise) {
                result.then(handleResult).catch(handleError);
              } else {
                handleResult(result);
              }
            } catch (err) { handleError(err); }

            /** @param {any} result */
            function handleResult(result) {
              state.json = JSON.stringify({ ok: true, result });
              instance.exports._continue(Number(cookie) || 0);
            }

            /** @param {any} err */
            function handleError(err) {
              state.json = JSON.stringify({ ok: false, error: String(err) });
              instance.exports._continue(Number(cookie) || 0);
            }
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
});
