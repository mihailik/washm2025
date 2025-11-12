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

Milestones & Implementation Plan (Stage Sequence)
---------------------------------------------
Phase A — Research and scaffolding (small quick stages)
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

Stage Breakdown (detailed)
-----------------------
This section expands the milestone list into explicit stages ready to be opened. Each stage should be small and testable. The files changed and acceptance criteria are described per stage.

Stage: Feature gating & parser opt-in (Feature Scaffolding)
   - Goal: Add a compile-time feature and opt-in wasmparser validator option for stack switching.
   - Files to change:
      - root Cargo.toml / workspace features (if workspace-level features are desired)
      - crates/parser/Cargo.toml: add feature flag `stack-switching` that enables the wasmparser stack_switching feature.
      - crates/parser/src/lib.rs: the Validator features toggling (currently `stack_switching: false`) should be enabled when the feature is on.
      - crates/tinywasm/Cargo.toml: add `stack-switching` feature, expose the feature to crates needing the runtime/parse support.
   - Tests:
      - Unit tests in `crates/parser/tests` to confirm that enabling/disabling the feature toggles wasmparser‘s validator flag.
   - Acceptance criteria:
      - The repository compiles with the new feature disabled (default).
      - Tests demonstrate that the wasmparser validator can be toggled on when `stack-switching` is enabled.

Stage: Parse scaffolding (Parse-only instruction scaffolding)
   - Goal: Add `Instruction` variants and `visit.rs` mapping so that the parser recognizes the new stack ops; do not implement runtime semantics yet.
   - Files to change:
      - crates/types/src/instructions.rs: add new variants (comment them with `// proposal` until names are finalized). Eg: `StackCreate`, `StackDrop`, `StackSwitch`, `StackSuspend`, `StackResume`.
      - crates/parser/src/visit.rs: add mappings from the wasmparser visitor opcodes into the new `Instruction` variants; gate these mappings behind the `stack-switching` feature.
      - crates/parser/src/lib.rs: ensure `Validator` feature gating works for parse tests.
   - Tests:
      - Parse-only tests that assert a module containing the new opcodes binds/validates when `stack-switching` is enabled, and fails when it's disabled.
   - Acceptance criteria:
      - Parser accepts a minimal module with the new instructions with the feature enabled.
      - Minimal test coverage exists for compliance with wasmparser validation expectations.

Stage: Runtime stubs (Runtime stubs - no semantics yet)
   - Goal: Emit the new opcodes as `Instruction` during parsing and add placeholder opcode handling in the `Executor` that traps or returns an informative `UnsupportedFeature` error while preserving program correctness in the presence of the opcode.
   - Files to change:
      - crates/tinywasm/src/interpreter/executor.rs: add match arms for the new `Instruction` variants; these should currently return `Error::UnsupportedFeature("stack switching opcode unimplemented")` or trap.
   - Tests:
      - Execute a module containing the new opcodes; it should validate and then trap at run time with a descriptive message.
   - Acceptance criteria:
      - The project compiles with the new matching arms.
      - Tests assert that running these opcodes traps with a consistent error message.

Stage: Store registry & StackInstance scaffolding (Store-level stack registry)
   - Goal: Add a `StackInstance` (aka TaskInstance) and a `stacks` registry in `Store` that can hold and manage multiple `Stack` instances.
   - Files to change:
      - crates/tinywasm/src/store/mod.rs:
         - Add `pub(crate) stacks: Vec<Option<StackInstance>>` to `StoreData`.
         - Add a `StackInstance` type that wraps `interpreter::stack::Stack` plus additional metadata (owner module, state, optional bound handle types, resource usage).
         - Implement helper functions `add_stack`, `take_stack`, `replace_stack`, `get_stack`, `get_stack_mut`, `len` and `free_stack`.
      - crates/tinywasm/src/lib.rs or types: add `StackAddr` and the API for host interaction if required.
   - Tests:
      - Unit tests for the store-level helper functions and state transitions (add -> take -> replace -> free).
   - Acceptance criteria:
      - No changes to the semantics of the current `CallStack`/`ValueStack` implementation.
      - Add/Take/Replace operations move the `Stack` instance in and out of the store without aliasing problems.
      - No borrow conflicts when performing basic operations.

Stage: Stack creation (stack.create implementation & runtime handle return)
   - Goal: Implement the `stack.create` instruction that allocates and registers a `StackInstance` in the `Store` and returns a `StackAddr` handle.
   - Files to change:
      - crates/tinywasm/src/interpreter/executor.rs: new handler `StackCreate` that allocates a new `StackInstance` and pushes a handle to the value stack.
      - crates/tinywasm/src/interpreter/stack/* — ensure constructor `Stack::new(call_frame, &store.config)` is called with an initial `CallFrame` or empty frame, as design dictates.
   - Tests:
      - Integration test that executes `stack.create` and receives a `StackAddr` which the test uses in subsequent stack ops.
   - Acceptance criteria:
      - `stack.create` produces a non-zero handle (`StackAddr`) and a `StackInstance` is visible in the `Store`.
      - The new `StackInstance` does not interfere with the `Store`'s existing runtime operations.

Stage: Stack switching (stack.switch / stack.enter)
   - Goal: Implement switching of the active execution stack with a minimal definition—move the `Stack` ownership into the running `Executor` and store back the active one when switching away.
   - Files to change:
      - crates/tinywasm/src/interpreter/executor.rs: implement a stack-swapping helper; add op handlers `StackSwitch` or `StackResume`.
      - crates/tinywasm/src/store/mod.rs: ensure `take_stack`/`replace_stack` supports the active swap semantics.
   - Tests:
      - Integration test: create two stacks, push different local values, switch between them, and validate that locals and value stacks are preserved across transitions.
   - Acceptance criteria:
      - Stack swap changes the Executor’s current `Stack` to the selected one and returns to the prior one upon a later `stack.switch` (or as per the proposal's semantics).

Stage: Suspend & resume (suspend/resume semantics & host-safe rules)
   - Goal: Implement `suspend` and `resume` semantics and rules around host functions and validation.
   - Files to change:
      - crates/tinywasm/src/interpreter/executor.rs: implement `StackSuspend`, `StackResume` — persist `CallFrame`, `BlockStack`, `ValueStack` and bundle them as `StackInstance` state.
      - crates/tinywasm/src/interpreter/stack/*: possibly add helper methods to serialize/deserialize a `Stack`'s runtime state if necessary.
      - crates/parser/src/visit.rs: validator changes to ban illegal patterns (cross-stack calls, suspend while executing host functions if invalid).
   - Tests:
      - Nested suspend/resume tests across function calls and host call boundaries.
      - Cases that are invalid should trap or fail validation as per proposal.
   - Acceptance criteria:
      - `suspend` stores the state of the current execution stack such that `resume` restores it.
      - Host functions interacting with suspension must match the proposal behavior (e.g. disallowing resume within host functions if the proposal disallows it.)

Stage: Validation & CI (validation & CI changes)
   - Goal: Add validation, CI gating, docs and example tests.
   - Files to change:
      - crates/parser/src/lib.rs / crated parser validation for new opcodes.
      - crates/tinywasm/tests/generated: add WAT examples (create, switch, suspend, resume; invalid resume scenarios).
      - CI pipeline: add `stack-switching` feature job to test the new feature set.
   - Tests:
      - Full CI job runs feature on & executes all stack-switching related tests.
   - Acceptance criteria:
      - Tests pass on CI when feature is enabled.
      - No regressions in default tests when the feature is off.

Implementation notes & borrow-checker tips
---------------------------------------
- Keep `CallStack` and `ValueStack` unchanged — they're plain owned data and behave correctly per-instance.
- Use a `Vec<Option<StackInstance>>` in `Store` to enable moving stacks in/out safely. `Option::take()` is the easiest way to move ownership of a `StackInstance` out of `Store` without borrow conflicts.
- When the `Executor` needs to switch stacks, use `store.take_stack(target)` to acquire the target `StackInstance` and `store.replace_stack(current)` to put the old one back. Doing it this way keeps the borrow checker happy since only one `Stack` is mutable at a time.
- If a stage needs simultaneous mutable access to two distinct `StackInstances`, implement a `get_pair_mut` helper similar to existing `get_pair_mut` for memory and tables.
- Resource limits can be enforced per stack using `StackConfig` and constants like `MAX_CALL_STACK_SIZE` in `CallStack`.

Stage process & size guidelines
----------------------------
- Keep stages small and self-contained: parse-only, stubs, and store scaffolding are good incremental commits.
- Each stage should be accompanied by unit tests and a short, explicit acceptance criterion list in the stage description (see per-stage acceptance criteria above).
- Avoid runtime semantics in early stages. Early stages should be feature-flagged and compile without runtime semantics.

Open questions & follow-ups
--------------------------
- Precise host function semantics for suspension and resume (should be clarified before building deep host integration tests).
- Proposal instruction names and encodings should be pinned to the latest official sources before merging the Stage: Parse scaffolding.
- Consider a runtime

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
- Proposal changes: opcode names or semantic changes may land while the implementation is in progress. Mitigation: keep parser and mapping behind a cargo feature and split implementation into small stages with feature flags.
- Complexity: The runtime changes are invasive; to rollback we must keep changes behind the feature flag and revert staged changes.

Next Steps (Immediate)
----------------------
1. Create an initial Stage that adds the `docs/0-tinywasm-ssw.md` plan (this file). — DONE
2. Add a small parser change Stage to enable wasmparser `stack_switching` feature behind a cargo feature and add parse-only instruction mapping tests.
3. Add an incremental runtime Stage (stubs) and tests that confirm compile and trap.

Helpful References
------------------
- Known WebAssembly proposals repo and the wasmparser library for feature flags and the new operators.
- This repo's architecture: `crates/parser/*`, `crates/tinywasm/*`, `crates/types/*`.

Open Questions / Notes
---------------------
- Exact instruction names: we'll use the latest official proposal names (confirm before Phase B). If the proposal changes, map the original opcode names to the new names and record them.
- Host function interaction semantics when suspending inside a host function call need spec clarification.

If you want, I can open follow-up stages to:
1. Add the cargo feature `stack-switching` and enable wasmparser feature toggling in `crates/parser/src/lib.rs`.
2. Add `Instruction` enum entries and simple parsing & validation tests (no runtime VM semantics yet).

---
Plan authored and added to docs/0-tinywasm-ssw.md to serve as the working implementation plan.
