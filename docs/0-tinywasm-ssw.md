# 0-tinywasm-ssw: Plan for Task / Stack Switching (Stack Switching Proposal)

Status: Draft

Overview
--------
This document outlines a pragmatic, incremental plan to implement WebAssembly "stack switching" (aka task switching) support in tinywasm. The goal is to add parsed/validated support and runtime semantics so tinywasm can execute stack-switching programs correctly and safely, while preserving opt-in feature gating and backwards compatibility.

Scope
-----
- Parser/validator support (enable Wasm feature and parse proposal opcodes)
- Internal instruction representation (add new Instruction variants)
- Runtime (call stack, task stack structures, stack allocation, suspend/resume semantics)
- Tests (unit, interpreter, WAT/wast tests)
- CI and feature flags
- Documentation (this plan + in-code docs)

Assumptions
-----------
- The final instruction names and binary encodings may change (proposal state). Where names are uncertain, the plan uses placeholders and references the proposal semantics in comments. The parser uses wasmparser; that crate optionally enables new features and yields operators via visitor hooks.
- The implementation will be incremental: parse + no-op -> runtime stubs -> full semantics.
- Feature gating will be used so the implementation is opt-in until stable.
- Platform/embedding constraints (no host threading changes) are enforced; the initial implementation is single-threaded, embedder-only changes come later.

Contract (Inputs / Outputs / Success Criteria)
--------------------------------------------
- Input: A WASM module that uses stack-switching opcodes as defined by the proposal.
- Output: Parser accepts the module, and the interpreter executes the module behaviorally according to the proposal's semantics (suspend/resume, stack creation, switch, save/restore) in enabled configurations; in disabled mode, the module is rejected/validation fails.
- Success criteria:
  - Parser/validation accepts or rejects stack-switching modules depending on feature flag.
  - Interpreter defines and runs stack operations preserving correct call stack, local frames, and value stack.
  - Tests cover happy path, error cases, and host callbacks.
  - Performance pathology is reported and kept documented.

High-Level Design
-----------------
1. Parser/Validator
   - Enable stack-switching in the wasmparser Validator features, gated behind a tinywasm compile-time or run-time option.
   - Extend `crates/parser/src/visit.rs` to convert wasmparser stack-switching operators into `Instruction` variants (new enum entries in `tinywasm_types::Instruction`).

2. Internal Instruction Representation
   - Add new `Instruction` variants to `crates/types/src/instructions.rs` (e.g., StackCreate/StackDrop/StackSwitch/StackSave/StackRestore/Suspend/Resume—use official names once finalized).

3. Store & Runtime
   - Add a `Stacks` collection in the `Store` (e.g., `store.stacks`) for allocated stacks / tasks (addresses, allocated memory, type). New store object `StackInstance` or `TaskInstance`.
   - Implement a Task/Stack object holding its own call stack and value stack. This may either become a new top-level runtime object (with its own call frames) or be represented as serializable state that can be re-instated by `CallFrame::reuse_for` or similar.
   - Modify `interpreter/stack` modules (call stack, value stack, block stack) or add a `TaskStack` wrapper, so the executor can operate on the current active task stack. The `CallFrame` must reference which `Stack` it belongs to and allow saving/restoring state.

4. Executor & Instruction Semantics
   - Add handlers in `crates/tinywasm/src/interpreter/executor.rs` for the new opcodes.
   - `stack.create` (if present): allocate a new stack instance in the store; return a `StackAddr` / reference.
   - `stack.drop`: free stack resources (validate it's not the currently running stack, or define behavior per proposal).
   - `stack.switch` or `resume`: switch to run a different stack (update `Executor` active `cf` and `stack` references). Transfer local frames, value stack, and block stacks accordingly.
   - `stack.save` / `stack.restore` or `suspend` / `resume`: suspend the current stack (save the execution state, optionally allow host interaction) and resume another stack.
   - Consider separate `TaskState` enum (Running, Suspended, Dead) in store.

5. Host/Embedder & ABI
   - Define or adopt a minimal host ABI for starting/resuming stacks; ensure host function interactions are safe (e.g., disallow resuming from host function contexts that don't match the suspended stack requirements).
   - For stack switching that transfers control to host, consider a `HostSuspend` pattern where the host gets a handle for resumption.

6. Resource Management and Safety
   - Enforce per-stack limits (call stack depth, value stack size) with clear traps (`Trap::CallStackOverflow`, memory out-of-bounds traps, etc.).
   - Ensure stores free resources on drop and tests for memory leaks.

7. Tests
   - Parser/unit tests: accept minimal modules with stack-switching operators; ensure wasmparser validator flags are correct.
   - Interpreter tests: basic create and switch behavior, suspend/resume semantics, host tests for host callbacks while a stack is suspended, trap behavior.
   - WAT/WAST tests: add to `crates/tinywasm/tests/generated` and the `tests/testsuite` for the stack-switching scenarios.

8. CI & Feature Gate
   - Add an optional feature `stack-switching` (cargo feature or runtime flag).
   - Run related tests only when feature is enabled.
   - Provide a doc entry detailing how to enable/disable.

Files to Edit (Primary)
-----------------------
- crates/parser/src/lib.rs — enable wasmparser feature when the flag is set or as an optional feature.
- crates/parser/src/visit.rs — add conversion of new operators to `Instruction` variants.
- crates/types/src/instructions.rs — add new `Instruction` enum entries and conversion helpers.
- crates/tinywasm/src/interpreter/executor.rs — decode and handle new opcodes; add `suspend`/`resume` semantics.
- crates/tinywasm/src/interpreter/stack/* — new `TaskStack` or updated `CallStack`/`CallFrame` to support multiple stacks and saving/restoring.
- crates/tinywasm/src/store/* / crates/tinywasm/src/lib.rs — add `Stacks` collection and `StackInstance` struct.
- crates/tinywasm/tests/ — add unit tests and generated WAT tests.

Edge cases & Details to Consider
--------------------------------
- Host functions: how does suspend/resume behave if a host function is executing during suspension?
- Cross-stack function calls: are those allowed? If so, how do we preserve locals? If not, reject during validation / runtime.
- Tail call & return-call interactions with task switching need careful semantics.
- Memory access and lifetime; must ensure stack region and frame references don't lead to invalid memory accesses.
- Stack address leaks: resuming invalid or dropped stacks must trap.
- Concurrent stacks: the minimal implementation is single-threaded; thread-safe concurrency and shared stacks is deferred.

Milestones & Implementation Plan (PR Sequence)
---------------------------------------------
Phase A — Research and scaffolding (small quick PRs)
  1. Update `crates/parser/src/lib.rs` to accept stack-switching features when `stack-switching` cargo feature is enabled.
  2. Add `Instruction` variants required for minimal parse support (no runtime behavior yet). Add `visit.rs` mapping for those operators and unit tests that parse modules including the new ops (not enabled in default CI).
  3. Add a `feature: stack-switching` cargo feature and update `Cargo.toml` features lists as necessary.

Phase B — Temporary runtime stubs
  1. Add no-op or `trap` runtime handlers for each opcode so the interpreter compiles and when the opcode is executed it traps with a descriptive 'not yet implemented' error.
  2. Add unit tests for parser+runtime that assert the parse succeeds or trap behavior as per the feature flag.

Phase C — Basic runtime semantics
  1. Add `StackInstance` type in `Store` and support an allocation API (`stack.create`) that returns a stack handle address.
  2. Implement `stack.switch` / `stack.enter` that updates the current Executor to the given stack. Add minimal tests verifying control transfers and basic stack-local behavior.

Phase D — Suspend & Resume semantics
  1. Implement `suspend`: persist the current stack state (values, call frames, block frames) and mark it suspended.
  2. Implement `resume`/`stack.restore`: re-activate a stack and restore the active frame to continue execution.
  3. Add tests for nested suspend/resume across function calls and host interactions.

Phase E — Validation, safety & host integration
  1. Validation rules: disallow illegal suspend/resume patterns per proposal semantics.
  2. Confirm resource limits/trapping behavior.
  3. Add host function API for controlling stacks (if proposal requires), and tests.

Phase F — Performance & Documentation
  1. Benchmarks and microoptimizations.
  2. Documentation updates and examples.
  3. Final review and merge request.

Testing Strategy
----------------
- Unit tests in `tinywasm` for call-stack operations & stack instance lifecycle.
- Integration tests: minimal modules that create two stacks and switch between them. WAT/WAST tests should contain small examples with `suspend`/`resume` and expected stack values.
- Add tests for failure modes: illegal resume, re-using dropped stack handles.
- Add a fuzz/failing test for resource exhaustion (call stack overflow, memory bounds).

Acceptance Criteria
-------------------
1. Parser: With `stack-switching` enabled, the parser/validator accepts or rejects code in line with wasmparser and spec expectations.
2. Runtime: With feature on, the runtime can create stacks, switch to them, suspend and resume, while preserving locals and stack frames.
3. Tests: Unit and integration tests validate behavior; no regressions in existing core tests.
4. CI: A feature-gated job is added to run stack-switching tests when `stack-switching` feature is set.

Risks & Rollbacks
-----------------
- Proposal changes: opcode names or semantic changes may land while the implementation is in progress. Mitigation: keep parser and mapping behind a cargo feature and split implementation into small PRs with feature flags.
- Complexity: The runtime changes are invasive; to rollback we must keep changes behind the feature flag and revert staged PRs.

Next Steps (Immediate)
----------------------
1. Create an initial PR that adds the `docs/0-tinywasm-ssw.md` plan (this file). — DONE
2. Add a small parser change PR to enable wasmparser `stack_switching` feature behind a cargo feature and add parse-only instruction mapping tests.
3. Add an incremental runtime PR (stubs) and tests that confirm compile and trap.

Helpful References
------------------
- Known WebAssembly proposals repo and the wasmparser library for feature flags and the new operators.
- This repo's architecture: `crates/parser/*`, `crates/tinywasm/*`, `crates/types/*`.

Open Questions / Notes
---------------------
- Exact instruction names: we'll use the latest official proposal names (confirm before Phase B). If the proposal changes, map the original opcode names to the new names and record them.
- Host function interaction semantics when suspending inside a host function call need spec clarification.

If you want, I can open follow-up PRs to:
1. Add the cargo feature `stack-switching` and enable wasmparser feature toggling in `crates/parser/src/lib.rs`.
2. Add `Instruction` enum entries and simple parsing & validation tests (no runtime VM semantics yet).

---
Plan authored and added to docs/0-tinywasm-ssw.md to serve as the working implementation plan.
