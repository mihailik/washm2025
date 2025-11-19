# Architectural Decisions & Open Questions

## 1. WASI Reactor Pattern
**Decision:** Axy will be implemented as a **WASI Reactor** (Library), not a Command.
**Reasoning:** The Command pattern (`_start`) is blocking. If the REPL blocks in `_start`, the Host (Node.js) cannot process async events or drive the event loop, leading to deadlocks.
**Implication:**
- The "REPL" loop logic moves to the Host (Node.js).
- Axy exports `axy_submit(ptr, len)` to receive commands.
- Axy exports `_initialize` (standard Reactor init).

## 2. TTY Handling in Reactor Mode
**Question:** How does `uutils` handle `stdin` if the Host is driving the input?
**Analysis:**
- If `uutils` commands (like `cat`) try to read from `stdin` (fd 0), they might block.
- **Option A:** Host buffers input and writes to a pipe connected to fd 0 before calling `axy_submit`.
- **Option B:** Axy uses a custom `stdin` implementation that calls back to the Host for more data (async read).
**Current Plan:** Start with **Option A** (simpler). The Host reads a line, passes it to `axy_submit`. If the command needs more input, it's a future problem (Phase 3).

## 3. `axy_submit` Signature
**Decision:** `(func (param i32 i32)) -> (result i32)`
- `ptr` (i32): Pointer to the command string in WASM memory.
- `len` (i32): Length of the command string.
- Returns: Exit code (0 for success, non-zero for error).

## 4. `proc_exit` Behavior
**Decision:** Axy should **not** call `proc_exit` to end the session.
- `proc_exit` terminates the entire WASM instance.
- Instead, `axy_submit` should return a specific exit code (e.g., `0`) to indicate "done with this command", and a special code (or just state) if the user wants to "exit" the shell.
- The Host decides when to terminate the instance.
