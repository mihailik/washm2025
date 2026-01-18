# 🚀 Project Axy: A Strategic Plan for the WASI Shell

**Mission:** To create the reference implementation for a secure, high-performance, and portable systems shell for the WebAssembly (WASI) ecosystem. Axy will fuse a modern, asynchronous REPL (the "Brush" head, provided from `brush-fork/`) with a memory-safe, synchronous core (the "uutils" library) via an internal function-call architecture — adopting the Brush shell as the canonical front-end for portability and WASI fitness.

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

## Phase 2: The Asynchronous Head (Brush Integration)

**Goal:** Integrate the user-friendly, asynchronous REPL features from Brush to create a modern, non-blocking user experience.

| Crucial Action | Description | Perfection Metric |
| :--- | :--- | :--- |
| **1. Brush Component Isolation** | **Isolate and adapt** the core REPL components from Brush (in `brush-fork/`) and the UX ecosystem: the parser, history manager, prompt rendering, and completion engine — remove Unix-specific dependencies and make them async-friendly. | Port the useful pieces to idiomatic, `unsafe`-free Rust and compile them as part of the Axy crate.
| **2. Async REPL Architecture** | Re-architect the Phase 1 REPL to be **asynchronous**. The main loop now `await`s user input, while background tasks (like prompt generation) can run concurrently. | The Axy prompt can display (for example) `git` status without blocking user input. |
| **3. Feature Integration** | Wire the asynchronous REPL to the **synchronous command dispatcher** from Phase 1. The shell *feels* fast, but the execution of `uutils` commands remains simple, blocking function calls. | The shell provides full syntax highlighting and tab-completion for its *internal* `uutils` commands. |



**Perfection Milestone (P2):** The Axy shell **feels** like a modern, responsive shell — a Brush-centric async head on a `uutils` sync core.

---
