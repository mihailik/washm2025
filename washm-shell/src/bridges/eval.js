// @ts-check
/// <reference path="../globals.d.ts" />

const evalState = new Map();

// Eval bridge: react to assignment of Uint8Array to '<cookie>.js' by evaluating and storing result in '<cookie>.json'
const evalProxyTarget = {};
globalThis.mounted.eval = new Proxy(evalProxyTarget, {
  get(target, prop) {
    if (typeof prop === 'string') {
      const parts = prop.split('.');
      const cookie = parts[0];
      const ext = parts[1];
      if (!evalState.has(cookie)) evalState.set(cookie, { json: null });
      const state = evalState.get(cookie);
      if (ext === 'json') {
        return state.json || '';
      }
    }
    return Reflect.get(target, prop);
  },
  set(target, prop, value) {
    if (typeof prop === 'string') {
      const parts = prop.split('.');
      const cookie = parts[0];
      const ext = parts[1];
      if (ext === 'js' && value instanceof Uint8Array) {
        if (!evalState.has(cookie)) evalState.set(cookie, { json: null });
        const state = evalState.get(cookie);
        const jsText = new TextDecoder().decode(value);
        try {
          let result = eval(jsText);
          if (result instanceof Promise) {
            result.then(handleResult).catch(handleError);
          } else {
            handleResult(result);
          }
        } catch (err) { handleError(err); }

        function handleResult(result) {
          state.json = JSON.stringify({ ok: true, result });
          try { instance.exports._continue(Number(cookie) || 0); } catch(e){}
        }
        function handleError(err) {
          state.json = JSON.stringify({ ok: false, error: String(err) });
          try { instance.exports._continue(Number(cookie) || 0); } catch(e){}
        }
        return true;
      }
    }
    return Reflect.set(target, prop, value);
  }
});
