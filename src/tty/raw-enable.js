// raw-enable.js
// Enables raw mode.
// This is a stub. In a real implementation, this would call the host's setRawMode.
try {
    if (process.stdin.setRawMode) {
        process.stdin.setRawMode(true);
    }
    return { ok: true };
} catch (e) {
    return { ok: false, error: e.toString() };
}
