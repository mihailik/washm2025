// raw-disable.js
try {
    if (process.stdin.setRawMode) {
        process.stdin.setRawMode(false);
    }
    return { ok: true };
} catch (e) {
    return { ok: false, error: e.toString() };
}
