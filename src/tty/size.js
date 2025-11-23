// size.js
try {
    return { ok: true, cols: process.stdout.columns, rows: process.stdout.rows };
} catch (e) {
    return { ok: false, error: e.toString() };
}
