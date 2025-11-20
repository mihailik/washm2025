# 🚀 Project Axy: A Strategic Plan for the WASI Shell

**Mission:** To create the reference implementation for a secure, high-performance, and portable systems shell for the WebAssembly (WASI) ecosystem. Axy will fuse a modern, asynchronous REPL (the "Ion-Lite" head) with a memory-safe, synchronous core (the "uutils" library) via an internal function-call architecture — but replace Fish with Ion as the canonical front-end for portability and WASI fitness.

---

## Phase 1: The Synchronous Foundation (The Monolith)

**Goal:** Prove the "axing" concept. Create a minimal, synchronous REPL that executes a library of internal `uutils` commands within a single WASM module.

| Crucial Action | Description | Perfection Metric |
| :--- | :--- | :--- |
| **1. `uutils` as a Library** | Import `uutils/coreutils` not as binaries, but as a Rust **library crate**. | `cargo build` produces a single library, not 100+ binaries. |
| **2. Guest REPL Entrypoint** | The guest runs its own REPL using standard WASI entrypoints (`_start` / `_initialize`), and manages stdin via WASI I/O. | The Host is passive and must not invoke guest REPL logic; guests write requests to the VFS (`/dev/host/eval`) when they need host services. |
| **3. Internal Command Dispatcher** | Implement the core `eval` function. This must be a `match` statement or `HashMap` that maps a string (e.g., "ls") directly to an internal Rust function call (e.g., `uutils::ls::main(args)`). | **Zero calls** to `std::process::Command`. All execution is a function call within the same process. |
| **4. WASI-Reactor Compilation** | Compile the entire monolith (`REPL + uutils`) to a single `wasm32-wasi` **reactor** module (library). | The `.wasm` file exports standard WASI init (`_start` / `_initialize`) and the single custom export `_continue(cookie)`; the Host is a passive launcher. |

**Perfection Milestone (P1):** A working but "dumb" Axy shell that has **successfully axed `fork/exec`**. It proves the core, tightly-coupled architecture works for core utilities.

> Canonical API: The only required non-WASI export in the minimal launcher is `_continue(cookie: i32)`. The host must not write to WASM memory or call guest allocators. Any other host-facing exports are either optional compatibility shims or deprecated.

---

## Phase 2: The Asynchronous Head (Ion-Lite Integration)

**Goal:** Integrate the user-friendly, asynchronous REPL features from Fish to create a modern, non-blocking user experience.

| Crucial Action | Description | Perfection Metric |
| :--- | :--- | :--- |
| **1. Ion Component Isolation** | **Isolate and adapt** the core REPL components from Ion and the UX ecosystem: the parser, history manager, prompt rendering, and completion engine — remove Unix-specific dependencies and make them async-friendly. | Port the useful pieces to idiomatic, `unsafe`-free Rust and compile them as part of the Axy crate.
| **2. Async REPL Architecture** | Re-architect the Phase 1 REPL to be **asynchronous**. The main loop now `await`s user input, while background tasks (like prompt generation) can run concurrently. | The Axy prompt can display (for example) `git` status without blocking user input. |
| **3. Feature Integration** | Wire the asynchronous REPL to the **synchronous command dispatcher** from Phase 1. The shell *feels* fast, but the execution of `uutils` commands remains simple, blocking function calls. | The shell provides full syntax highlighting and tab-completion for its *internal* `uutils` commands. |



**Perfection Milestone (P2):** The Axy shell **feels** like a modern, responsive shell — an `Ion`-centric async head on a `uutils` sync core.

---

## Phase 3: The WASI Component Model (External Execution)

**Goal:** Solve the external command problem *without* `fork/exec`, using the official WASI standards for secure, sandboxed execution.

> Note: The "secure, sandboxed execution" described here refers explicitly to running *external* WASM components using the WASI Component Model and is not a proposal for the canonical `host_eval` JS execution flow. The canonical `host_eval` flow remains: guests write raw JS to `/dev/host/eval`, and the launcher executes this JS in the host runtime scope.

| Crucial Action | Description | Perfection Metric |
| :--- | :--- | :--- |
| **1. Adopt the Component Model** | Re-architect Axy to be a **WASM Component**. Its API (imports/exports) must be defined using **WIT (WASI Interface Types)**. | Axy is no longer a "core module" but a "component" that imports `wasi:cli/run` and `wasi:io/streams`. |
| **2. Implement External Spawner** | The command dispatcher is upgraded. If a command is *not* in the internal `uutils` map, Axy will use the `wasi:cli/run` import to request the host runtime to **instantiate and run** the target command as a *separate* WASM component. | Axy can run `git.wasm` (an external component) by delegating the execution to the WASI runtime. |
| **3. Master Pipelining** | Implement the `|` (pipe) operator. This must **not** use OS pipes. Axy must use `wasi:io/streams` to create an in-memory pipe, linking the `stdout` stream of the first component to the `stdin` stream of the second. | The command `cat file.txt | grep "test"` works, where `cat` is an internal `uutils` function and `grep` is an external component, with I/O piped via WASI. |



**Perfection Milestone (P3):** Axy is the **reference implementation** for a `wasi-cli` shell. It seamlessly blends high-speed internal commands with secure, sandboxed external component execution for *external* WASM components (not the host's canonical JS eval path).

---

## Phase 4: Optimization & Hardening (The "Burner Down")

**Goal:** Ensure Axy is the tightest, fastest, and most "uncorrupted" shell, fit for the most secure embedded environments.

| Crucial Action | Description | Perfection Metric |
| :--- | :--- | :--- |
| **1. `no_std` Investigation** | As the "perfection" goal, investigate a **`no_std` build**. This would require `uutils` and the REPL to be alloc-only, removing all OS dependencies. | A `no_std`-compatible build of Axy that can run on a microcontroller or in-browser. |
| **2. Binary Size Optimization** | Relentlessly apply `wasm-opt`, LTO (Link Time Optimization), and code-stripping to minimize the final `.wasm` file size. | The Axy monolith (REPL + all `uutils`) is significantly smaller than a traditional C-based `busybox`. |
| **3. Security & Capability Audit** | Fuzz the parser and command dispatcher. **Verify** that Axy *never* attempts to access a file or network socket that wasn't explicitly granted to it by the host runtime. Audits validate that Axy respects host-provided capabilities and does not attempt to escalate privileges. | Axy passes a full security audit, proving it is compliant with the WASI capability-based security model. |

**Perfection Milestone (P4):** Axy is released as the gold standard for secure, embedded, and high-performance systems interaction in the WebAssembly era; guests that write JS to `/dev/host/eval` will have their code executed by the host according to the canonical VFS eval flow.