# Vision: The Decoupled Self-Instrumenting Test Runner

This document outlines the architecture for `washm-test`, a specialized toolchain for creating self-instrumenting WebAssembly binaries. Unlike traditional coverage tools that rely on language-specific compiler flags (like `-C instrument-coverage`) or heavy runtime engines, this system operates directly on the WASM binary, making it language-agnostic and engine-independent.

## Core Philosophy: "The Binary is the Source of Truth"

The system does not rely on the `washm-engine` IR. It treats the input WASM binary as a stream of instructions and the DWARF debug sections as the map. It performs surgical, single-pass transformations to inject observability without altering the semantic behavior of the guest program.

## Architecture Components

### 1. The Distiller (Rust CLI)
A standalone binary tool that consumes a standard `.wasm` file (compiled with debug info) and emits a `.distilled.wasm` file.

*   **Dependencies**: `walrus`, `gimli`, `wasmparser` (optional but kept for reference). **NO** dependence on `washm-engine`.
*   **Role**:
    1.  **Ingestion (Walrus)**: Loads the binary into a robust, modifiable standard IR.
    2.  **Parse DWARF**: Uses `gimli` to identify executable source lines from the raw binary sections.
    3.  **Scan for Frontiers**: Locates and patches memory layout globals (`__heap_base`, `__data_end`).
    4.  **Instrument**: Injects probes at basic block boundaries corresponding to source lines using `walrus`'s modification API.
    5.  **Meta-Data**: Emits a sidecar JSON mapping binary offsets to source locations.

### 2. The Infiltrator (Rust Macro / Guest Runtime)
A lightweight Rust library linked into the guest test suite.

*   **Role**:
    1.  Defines the common symbols (`__WASHM_COV_PTR`) via `no_mangle` static variables.
    2.  Provides the `init!` macro to satisfy any linker requirements.
    3.  **Patch Target**: These symbols act as "landing pads" that the Distiller finds and initializes to point to the created DMZ.

### 3. The Orchestrator (Node.js Host)
A test runner that executes the distilled binary in a standard runtime (Node.js or embedded Wasmtime or Wasmer or similar).

*   **Role**:
    1.  Instantiates the WASM module.
    2.  Mounts the pre-allocated coverage buffer (which the Distiller has baked into the binary).
    3.  Executes tests.
    4.  Reads the binary memory dump on completion and correlates it with the JSON mapping to generate reports (LCOV/HTML).

---

## Technical Deep Dive: The Frontier Shift

To ensure memory safety without a complex allocator in the guest, we use the **Frontier Shift** technique to carve out a "Demilitarized Zone" (DMZ) for coverage data.

### The Problem
Injecting a coverage buffer (likely ~1-10KB) into a compiled binary risks colliding with the guest's heap allocator (e.g., `dlmalloc` or `wee_alloc`), which expects to own all memory from `__heap_base` upwards.

### The Solution
We perform a surgical relocation of the heap boundary using `walrus`.

1.  **Analyze Memory Layout**:
    The Distiller scans the `Export` section for `__heap_base` (provided by `wasm-ld`).
    *   *Example*: `__heap_base` is at `0x100000` (1MB).

2.  **Calculate Frontier**:
    We need `N` bytes for our coverage map (where `N` = Total Probes).
    *   *New Base* = `0x100000` + `N`.
    *   *Alignment*: Align `New Base` to 16 bytes.

3.  **Patch the Global**:
    The Distiller identifies the `Global` ID for `__heap_base` and updates its `Local` initializer value to the *New Base*.
    *   *Result*: The guest allocator receives the shifted start address.

4.  **Inject Active Data Segment**:
    We add a new `Data` segment to the module.
    *   *Offset*: `i32.const 0x100000` (The Old Base).
    *   *Content*: A zero-initialized buffer of size `N`.
    *   *Effect*: The WASM runtime initializes our coverage zone automatically at instantiation.

5.  **Memory Growth**:
    `walrus` automatically handles `Memory.initial` updates if the new data segment pushes past the current memory limits.

---

## Instrumentation Strategy: Walrus-Based Injection

By using `walrus`, we gain robust handling of instruction offsets and block structures.

1.  **DWARF Loading**:
    We use `gimli` to load `.debug_line`. We map `TextOffset -> SourceLocation`.

2.  **Probe Injection**:
    We iterate over `walrus::LocalFunction` bodies.
    *   For each instruction sequence (basic block), we check if the DWARF map indicates a new source line.
    *   **Injection**: We prepend a probe sequence to the block.
    
    ```wasm
    ;; Load base address (Patched Global)
    global.get $__WASHM_COV_PTR
    
    ;; Add Probe Offset
    i32.const {PROBE_ID}
    i32.add
    
    ;; Load current byte
    tee_local $temp ;; Optimization if needed
    i32.load8_u 0 0
    
    ;; Increment
    i32.const 1
    i32.add
    
    ;; Store back
    i32.store8 0 0
    ```

3.  **Global Patching**:
    Instead of injecting a new export, the Distiller looks for the `__WASHM_COV_PTR` global (exported by the guest via the `init!` macro) and patches its initializer value to point to the start of our DMZ data segment.

## Implementation Roadmap

1.  **Phase 1: The Clean Slate**
    *   Initialize `washm-test` with `walrus`, `gimli`, `object`.
    *   Ensure **zero** dependencies on `washm-engine`.

2.  **Phase 2: The Frontier Shifter**
    *   Implement `__heap_base` discovery using `walrus`.
    *   Implement Data Segment injection (`module.data.add`).
    *   Verify `dlmalloc` respects the shift.

3.  **Phase 3: The Mapper**
    *   Implement DWARF parsing to identifying executable code regions.
    *   Correlate Walrus `InstrSeqId` or instruction offsets to Rust source lines.

4.  **Phase 4: The Injector**
    *   Implement the instruction splicer.
    *   Patch `__WASHM_COV_PTR`.

5.  **Phase 5: The Report**
    *   Update `runner.js` to read the DMZ from `instance.exports.memory.buffer` and dump it to JSON.
