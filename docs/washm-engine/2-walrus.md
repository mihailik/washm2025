# Bridging the Gap: From Binaryen Power to Walrus DX

## The Objective
`washm-engine` has successfully ported the optimization power of **Binaryen** (Tree IR, Asyncify, SSA-based passes). However, to displace **Walrus** as the preferred Rust tool for mid-level manipulation, it must transition from a "Batch Optimizer" to a "Surgical Tool."

This proposal outlines the shift toward **"Walrus DX + Binaryen Intelligence."**

---

## 1. Generational ID Management (The "Stable Index" Strategy)

Current `washm` IR uses raw `u32` for indices (functions, locals, types). This makes manual manipulation fragile: deleting function #1 invalidates all references to function #2.

### The Proposal:
Replace `Vec<T>` structures in the `Module` with an **ID-Arena**.
*   **Infrastructure:** Use a generational index-based arena.
*   **New Types:** `FuncId`, `LocalId`, `GlobalId`, `TypeId`.

### The "How":
- Modify `Module` internals:
  ```rust
  pub struct Module<'a> {
      pub functions: Arena<Function<'a>>, // id_arena::Arena or internal equivalent
      // ...
  }
  ```
- All IR nodes (like `ExpressionKind::Call`) should store `FuncId` instead of `&str` (name) or `u32` (index). This ensures references stay valid during passes like **Duplicate Function Elimination**.

---

## 2. The Fluent Mutation API (Surgical Edits)

Walrus excels at "wrapping" or "snipping" parts of a module with minimal code.

### The Proposal:
Implement a **Fluent Builder** that abstracts the complexity of `IrBuilder` and `Module` synchronization.

### The "How":
Create a high-level `Editor` trait or struct:
```rust
module.edit_function(main_id)
      .intercept_call(logger_id) // Internal logic prepends a call
      .instrument_locals();      // Calls our existing pass internally
```

---

## 3. Metadata & Custom Section Preservation

Walrus treats Wasm as a transparent proxy. `washm` currently treats it as a "lossy transformation."

### The Proposal:
Implement a **Custom Section Registry** to ensure round-trip parity for non-code data.

### The "How":
- **BinaryReader:** Instead of ignoring unknown payloads, store them in `Module.custom_sections: Vec<CustomSection>`.
- **Specialized Parsers:** Automatically parse the `name` and `producers` sections into structured fields in the IR rather than treating them as opaque.
- **BinaryWriter:** Emit these sections in the correct order, ensuring the resulting binary is recognizable by `wasm-tools` and debuggers.

---

## 4. Precision-Engineered DWARF Support

This is the "Killer App." We must ensure that optimizing a module doesn't make it impossible to debug.

### The Proposal:
Use the existing **Tree IR Annotations** to carry DWARF Source Spans from input to output.

### The "How":
1.  **Ingestion:** During binary parsing, extract `.debug_line` information and attach it to the `ExprRef` as an `Annotation::Location`.
2.  **Propagation:** Ensure every optimization pass (e.g., `Inliner`) clones or merges these location annotations.
3.  **Emission:** The `BinaryWriter` must use a library like `gimli` to reconstruct a valid DWARF section mapping the *new* instruction offsets to the *original* source locations stored in the annotations.

---

## The Win Condition

By implementing the above, `washm-engine` becomes the undisputed king of the Rust Wasm ecosystem. It will be the only tool where a developer can:
1.  **Surgically wrap** an export (Walrus style).
2.  **Run a rigorous Asyncify pass** (Binaryen style).
3.  **Preserve DWARF debug info** throughout the entire process.
