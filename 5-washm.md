# WASHM Branding and Crate Consolidation Plan

## Objective
To transition the Rust port of Binaryen to the new `WASHM` brand, consolidating core logic into a single `washm-engine` crate and providing a compatible CLI as `washm-opt`. This plan aims for a pragmatic and realistic approach, focusing on essential steps for a successful launch.

## 1. Branding & Naming Convention

*   **New Brand Name:** `WASHM`.
*   **Crate Naming:** All Rust crates will adopt the `washm-*` prefix.
    *   `washm-engine`: Core library.
    *   `washm-opt`: Main CLI (optimization tool).
    *   `washm-macros`: Procedural macro helper (for test infrastructure).
    *   (Future optional CLIs: `washm-wat2wasm`, `washm-wasm2wat`, etc.)

## 2. Crate Consolidation Strategy

The goal is to consolidate the existing `binaryen-core`, `binaryen-ir`, `binaryen-decompile`, `binaryen-support`, and parts of `binaryen-ffi` into a single `washm-engine` crate.

### 2.1. `washm-engine` Crate (Library)

**Contents:** This will be the foundational library, containing all WebAssembly Intermediate Representation (IR), parsing, emission, analysis, and transformation logic.

*   **Core IR Definitions:**
    *   Module structure, functions, types, expressions, literals, etc. (migrated from `binaryen-core`, `binaryen-ir/src/module`, `binaryen-ir/src/expression`).
*   **WASM Binary/Text I/O:**
    *   Logic for reading and writing `.wasm` (binary) and `.wat` (text) formats. (migrated from `binaryen-ir/src/wasm_parser`, `binaryen-ir/src/wasm_printer`).
*   **Passes & Optimizations:**
    *   All optimization and transformation passes (e.g., `SimplifyGlobals`, `DCE`, `Inliner`).
    *   The pass manager infrastructure.
    *   All analysis utilities (e.g., `GlobalAnalysis`, `CallGraph`).
    *   (Migrated from `binaryen-ir/src/passes`, `binaryen-ir/src/analysis`).
*   **Decompilation Logic:**
    *   The core algorithms and structures for decompiling WebAssembly into higher-level representations (migrated from `binaryen-decompile`).
*   **Support Utilities:**
    *   Common helper traits, error types, utility functions (migrated from `binaryen-support`).

**Internal Structure (within `washm-engine/src`):**
A clear module hierarchy will be maintained for organization:
```
washm-engine/src/
├── lib.rs
├── ir/                 // Core IR definitions (module, expression, types, etc.)
├── wasm/               // Binary/text format I/O (parser, printer)
├── passes/             // All transformation and optimization passes
├── analysis/           // All analysis components (call graph, global uses, etc.)
├── decompile/          // Decompilation specific logic
└── util/               // General utilities and helpers
```

### 2.2. `washm-opt` Crate (Binary CLI)

**Purpose:** To provide a command-line interface compatible with Binaryen's `wasm-opt` for applying optimizations.

*   **Dependency:** Will depend on `washm-engine`.
*   **Functionality:**
    *   Parses command-line arguments (e.g., using `clap`).
    *   Loads WASM/WAT modules using `washm-engine`'s I/O capabilities.
    *   Invokes optimization passes and pass management provided by `washm-engine`.
    *   Outputs optimized WASM/WAT.

### 2.3. `washm-macros` Crate (Procedural Macro)

**Purpose:** Provides procedural macros for build-time code generation, specifically for automated test discovery (e.g., `triad_tests` for decompiler tests).

*   **Dependency:** Will *not* depend on `washm-engine` for its core macro logic, but the generated code will likely use `washm-engine` types/functions.
*   **Mandatory Status:** Only mandatory if the specific test automation it provides is desired. It's part of the test infrastructure, not the runtime product.

### 2.4. Decompiler CLI (Decision)

*   **Current Decision:** Postpone the creation of a dedicated decompiler CLI for `WASHM`. The core decompiler logic will reside within `washm-engine` as a library feature. This simplifies the initial launch and focuses efforts.

### 2.5. `washm-ffi` Crate (Future Consideration)

*   **Recommendation:** Keep `binaryen-ffi` separate and rename it to `washm-ffi`. It should depend on `washm-engine`. This ensures a clear separation for FFI bindings, allowing the core engine to remain pure Rust and simplifying `build.rs` logic for C interop. This can be addressed once `washm-engine` and `washm-opt` are stable.

## 3. High-Level Migration Steps

1.  **Create New Crate Structure:**
    *   Create `washm-engine`, `washm-opt`, `washm-macros` directories and their `Cargo.toml` files under `rust/`.
2.  **Migrate Code to `washm-engine`:**
    *   Carefully move relevant source files from `binaryen-core`, `binaryen-ir`, `binaryen-decompile`, `binaryen-support` into their appropriate submodules within `washm-engine/src`.
    *   Update `lib.rs` in `washm-engine` to properly re-export modules.
3.  **Update `Cargo.toml` files:**
    *   Rename all `binaryen-*` to `washm-*` in `name` fields.
    *   Adjust `dependencies` within `washm-opt`, `washm-macros`, and `washm-ffi` (if created) to point to `washm-engine` or other `washm-*` crates.
4.  **Refactor `use` Statements:**
    *   Perform a global find-and-replace for `binaryen_*` imports to `washm_engine::*` or `washm_macros::*` as appropriate.
5.  **Migrate `washm-opt` Logic:**
    *   Move the CLI entry point and argument parsing logic from `binaryen-tools` into `washm-opt/src/main.rs`.
    *   Adapt it to use `washm-engine`'s API.
6.  **Test Infrastructure:**
    *   Ensure `washm-macros` continues to function correctly for decompiler tests by updating paths and crate names.
    *   Migrate existing tests, ensuring they pass with the new structure.
7.  **Documentation Update:**
    *   Update all `README.md` files.
    *   Review and update any project-level documentation (`docs/`) to reflect the `WASHM` brand and new crate structure.
8.  **Comprehensive Testing:**
    *   Run the entire test suite.
    *   Perform integration testing to ensure `washm-opt` functions as expected.
    *   Cross-check outputs against original Binaryen where applicable.

## 4. Risks & Mitigations

*   **Broken References:** Extensive find-and-replace might miss some references.
    *   *Mitigation:* Thorough testing, IDE-assisted refactoring, and static analysis tools.
*   **Dependency Conflicts:** Merging crates might expose hidden dependency issues.
    *   *Mitigation:* Careful review of all `Cargo.toml` dependencies and `cargo clean && cargo build` frequently.
*   **Build System Complexity:** `build.rs` scripts for FFI might need careful migration.
    *   *Mitigation:* Prioritize `washm-engine` and `washm-opt` first, deferring `washm-ffi` for a second pass.

This plan provides a clear roadmap for the `WASHM` transition.