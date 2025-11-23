try {
  if (process.stdout.isTTY) {
    ({ ok: true, rows: process.stdout.rows, cols: process.stdout.columns })
  } else {
    ({ ok: false, error: "not a tty" })
  }
} catch (e) {
  ({ ok: false, error: String(e) })
}
