# Vision Document: Project "WASHM-std"

To build a truly custom system interface that supercedes WASI, we must move away from the "WASI-on-WASI" emulation mindset. Instead, we treat your custom host as a first-class operating system target within the Rust standard library.

This vision document outlines the transition from the generic `wasm32-unknown-unknown` baseline to a specialized, high-performance platform.


**Custom Platform Abstraction Layer (PAL) for Rust**

## 1. WHAT: The Objective

We are creating a custom **Platform Abstraction Layer (PAL)** for the Rust `std` library. Instead of the limited, POSIX-lite interface of WASI, we are wiring the standard library types (`File`, `TcpStream`, `Mutex`, `Thread`) directly to your proprietary host functions.

* **Platform:** `wasm32-washm-os` (Custom Target).
* **Scope:** Full `std` support including I/O, Networking, and Synchronization.
* **Differentiation:** No WASI middleman. Direct mapping to host-optimized syscalls.

## 2. WHY: The Motivation

WASI was designed for maximum portability, which often leads to "lowest common denominator" performance and API design.

* **Platform Affinity:** WASI abstracts away host-specific strengths (e.g., specialized ring buffers or asynchronous I/O models). Project WASHM-std links Rust directly to those strengths.
* **Functionality:** WASI Preview 1 lacks robust threading and socket control. By defining our own PAL, we expose your host's specific capabilities (like shared-memory atomics or custom hardware acceleration) via standard Rust syntax.
* **Security:** Removing the WASI layer reduces the attack surface and allows for a "capability-based" security model tailored specifically to your host's architecture.

## 3. HOW: Execution Strategy

We will bypass the pre-compiled `wasm32-wasip1` target and use the **Standard Library Patching** method. This allows us to define the "wirings" without needing a custom fork of the entire `rustc` compiler.

### Phase A: The Target Definition

We create a custom target JSON that describes the machine architecture but leaves the OS behavior "unspecified" to the compiler, allowing us to fill it in via `std`.

```json
{
  "llvm-target": "wasm32-unknown-unknown",
  "os": "washm",
  "env": "p1",
  "linker-flavor": "wasm-ld",
  "panic-strategy": "abort",
  "has-thread-local": true
}

```

### Phase B: The PAL Implementation (The "Wiring")

We create a local copy of the Rust `library/std/src/sys` directory. We ignore the `wasi` and `unix` folders and create a new `washm` folder.

**Key "Wiring" Modules:**

* **`sys/washm/io.rs`**: Maps `std::io` to your host's read/write functions.
* **`sys/washm/net.rs`**: Maps `std::net` to your host's socket syscalls.
* **`sys/washm/thread.rs`**: Implements `std::thread` using your host's threading primitives (e.g., `web-worker` style or green threads).
* **`sys/washm/alloc.rs`**: Connects the global allocator to your host's memory management.

### Phase C: The Build Pipeline

We use Cargo's `patch` and `build-std` features to force the compiler to use our custom PAL.

1. **Cargo.toml Patch:**
```toml
[patch.crates-io]
std = { path = "./rust-library/std" }

```


2. **The Compilation Command:**
```bash
cargo build -Z build-std=std,core,alloc --target wasm32-washm.json

```



---

## The Result

A `.wasm` binary that uses standard `println!`, `fs::File`, and `TcpStream`, but when inspected, contains **zero** `wasi_snapshot_preview1` imports. Instead, it contains clean imports from your own custom namespace (e.g., `zenith_sys_read`).

-----






### Vision Document: Project `washm-std`

**A Sovereign Platform Abstraction Layer for High-Performance Rust Wasm**

---

## 1. WHAT: The Objective

**`washm-std`** is a bespoke implementation of the Rust Standard Library’s **Platform Abstraction Layer (PAL)**. Unlike the generic `wasm32-unknown-unknown` (which lacks a functional `std`) or `wasm32-wasip1` (which is tethered to legacy POSIX-style imports), `washm-std` provides a direct, high-affinity bridge between Rust’s high-level types and a modern, feature-rich host interface.

We are not shimming WASI; we are replacing it. We are building a "native" experience for WebAssembly where the "OS" is your custom host.

---

## 2. WHY: The Motivation

The current Wasm ecosystem suffers from "The WASI Tax": performance overhead and API friction caused by translating modern host capabilities into 1980s-era syscall mentalities.

* **Platform Affinity:** Modern hosts often provide ring buffers, zero-copy memory mapping, and advanced async primitives. `washm-std` allows Rust to use these directly.
* **Feature Parity & Beyond:** WASI Preview 1 is notoriously limited (e.g., no robust networking or threading). `washm-std` allows us to implement `std::net`, `std::thread`, and `std::process` using your host's superior facilities.
* **Binary Efficiency:** By eliminating the WASI-libc dependency and the associated "translation glue," we produce leaner, more specialized `.wasm` binaries.
* **Developer Experience:** Developers use the standard `use std::...` syntax they know, but get the performance of a custom-tuned engine.

---

## 3. HOW: The Implementation Architecture

We will execute this by "de-coupling" the standard library from the official Rust distribution and re-coupling it to your host via the `library/std/src/sys` interface.

### A. The Target Specification

We define a custom target `wasm32-washm.json`. We set the `os` field to `"washm"`. This is critical: it tells the Rust compiler's conditional compilation logic (`#[cfg(target_os = "washm")]`) to look for our custom code paths.

### B. The PAL Structure (The "Wiring")

We create a new directory within the `std` source: `library/std/src/sys/pal/washm/`. This folder will contain the "Proprietary Logic" for every major `std` facility:

| Module | Responsibility | `washm-std` Implementation |
| --- | --- | --- |
| **`mod.rs`** | Entry point | Exports the sub-modules to the rest of `std`. |
| **`io.rs`** | Buffering & Syscalls | Direct calls to `washm_host_read` / `washm_host_write`. |
| **`net.rs`** | Sockets | Maps `TcpStream` to your host’s networking stack. |
| **`thread.rs`** | Concurrency | Uses your host's thread-spawning or worker logic. |
| **`time.rs`** | Clocks | High-precision host timestamps. |
| **`alloc.rs`** | Memory | Interfaces with your host's heap management. |

### C. The Build Pipeline (Surgical Injection)

We use the **Crate Patching** mechanism. This allows us to maintain a clean workspace without forking the entire `rustc` binary.

1. **Vendor `std`:** We pull the `library/std` source into your project repository.
2. **Redirecting via `Cargo.toml`:**
```toml
[patch.crates-io]
std = { path = "./washm-std/library/std" }

```


3. **Compilation via `build-std`:**
We trigger a re-compilation of our modified `std` using the unstable `build-std` flag, targeting our custom JSON spec.

---

## 4. Summary of the Vision

`washm-std` moves the "System" in WebAssembly System Interface from a generic, restrictive standard to a high-performance, developer-defined reality. It enables a "Rust-Native" host where the boundary between Guest and Host is as thin as a single function call.

**Next Step:** Would you like me to generate the directory structure and the initial `mod.rs` for the `pal/washm` directory to begin the wiring?