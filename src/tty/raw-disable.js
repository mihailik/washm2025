try {
  if (process.stdin.isTTY) {
    process.stdin.setRawMode(false);
    ({ ok: true })
  } else {
    ({ ok: false, error: "not a tty" })
  }
} catch (e) {
  ({ ok: false, error: String(e) })
}
