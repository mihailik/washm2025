# Warnings to tidy

These are the warnings we want to tidy, and detailed research into each one of them.

## Unexpected `cfg` condition values in `uucore`

**File:** [vendor/uucore/src/lib/lib.rs](../target/vendor/uucore/src/lib/lib.rs) (and others in `uucore`)

### Warning
```text
warning: unexpected `cfg` condition value: `fsxattr`
   --> src/../target/vendor/uucore/src/lib/lib.rs:125:17
    |
125 | #[cfg(all(unix, feature = "fsxattr"))]
    |                 ^^^^^^^^^^^^^^^^^^^
    = note: expected values for `feature` are: ...
    = help: consider adding `fsxattr` as a feature in `Cargo.toml`
```

### Analysis
This warning is triggered by Rust's `check-cfg` feature. Unlike typical dependencies, `uucore` is not being built as an external crate; instead, `build.rs` copies its source into `target/vendor/uucore` and it is included directly as a module in `washm` via a `#[path]` attribute in [src/lib.rs](../src/lib.rs). Because it is now part of the `washm` crate, the compiler validates its `#[cfg(feature = "...")]` attributes against the features defined in `washm`'s `Cargo.toml`. Since `washm` does not define features like `fsxattr` or `selinux`, the compiler flags them as unexpected.

Currently, `uucore` is integrated into the source tree but is not yet actively used by the `CoreutilsDispatch` logic in [src/coreutils_dispatch.rs](../src/coreutils_dispatch.rs), which still uses manual stubs. As we move towards a more complete coreutils integration, these warnings will persist unless the features are declared or the lint is suppressed.

### Suggestion
There are two primary ways to handle these warnings:

1.  **Declare the features in `Cargo.toml`**: We can add all the features used by `uucore` (e.g., `fsxattr`, `selinux`, `backup-control`) to the `[features]` section of our `Cargo.toml`. This is the "correct" way according to the compiler, as it makes the features known to the crate.
2.  **Suppress the lint in `Cargo.toml`**: We can add a lint configuration to `Cargo.toml` to explicitly allow these specific unexpected values or to disable the check for the `feature` cfg entirely within this crate.

Given that `uucore` is vendored and we are essentially "absorbing" it into `washm`, declaring the features or suppressing the lint at the crate level are both viable. Suppressing the lint is likely cleaner as it avoids cluttering our `[features]` list with dozens of flags we don't directly control.

### Resolution
We have opted to declare the features in [Cargo.toml](../Cargo.toml). This silences the warnings and provides a clear roadmap of the coreutils capabilities we intend to integrate. A note has been added to the `Cargo.toml` file explaining their relevance.

### Decision

Enabling them in Cargo.toml so they can be used later and resolving the warnings.

## Unused import in `jobs.rs`

**File:** [src/jobs.rs](../src/jobs.rs#L2)

### Warning
```text
warning: unused import: `std::ffi::OsString`
 --> src/jobs.rs:2:5
  |
2 | use std::ffi::OsString;
  |     ^^^^^^^^^^^^^^^^^^
```

### Analysis
The `OsString` import is a remnant of a previous implementation of `execute_command_sync` that lived in this file. Historical analysis shows that this function was used to bridge command execution to coreutils, but it was removed in commit `61d30494d2aa1c6928186c257b1546ad2d9a7c97` ("Working directory fixes.") when the project shifted to using `CoreutilsDispatch`.

The file `src/jobs.rs` itself appears to be a leftover from "Phase 2" of development (commit `7c4035c2f0e7586411a05be3da93d4239197d6fc`). While it defines a `JobManager`, the `brush-core` crate (which is vendored and transformed) provides its own `JobManager` which is what the shell actually uses.

### Suggestion
Since the entire file `src/jobs.rs` is currently unused by the rest of the project (though it is included as a module in `lib.rs`), the cleanest fix is to remove the unused import. If the `JobManager` in this file is intended to eventually replace or augment the one in `brush-core`, it should be kept but marked with `#[allow(dead_code)]`. However, given it's a duplicate of `brush-core` functionality, it might be better to eventually remove the file entirely if it's not needed. For now, simply removing the `OsString` import will silence the warning.

### Decision

Remove the unused jobs.rs file entirely since it's not used.

## Unused assignment in `stubs_impl.rs`

**File:** [src/stubs_impl.rs](../src/stubs_impl.rs#L293)

### Warning
```text
warning: value assigned to `exit_code` is never read
   --> src/stubs_impl.rs:293:29
    |
293 |         let mut exit_code = 0i32;
    |                             ^^^^
    = help: maybe it is overwritten before being read?
```

### Analysis
The variable `exit_code` is initialized to `0i32`, but it is always overwritten inside the `loop` before it is ever read. The loop only breaks when a status is successfully parsed, at which point `exit_code` is assigned a value from the status file.

### Suggestion
The initialization `0i32` can be removed. Instead, the variable can be declared without an initial value, or the loop can be refactored to return the exit code directly.

### Decision

Remove the initialiser. Exiting right out of the loop seems a bit audacious: there is a little bot more logic happening after the loop.

## Unused variable in `stubs_impl.rs`

**File:** [src/stubs_impl.rs](../src/stubs_impl.rs#L344)

### Warning
```text
warning: unused variable: `code`
   --> src/stubs_impl.rs:344:34
    |
344 |     fn create_exit_status(&self, code: i32) -> std::io::Result<std::process::ExitStatus> {
    |                                  ^^^^ help: if this is intentional, prefix it with an underscore: `_code`
```

### Analysis
The `create_exit_status` function is a stub that attempts to construct a `std::process::ExitStatus` on WASM, which is difficult because the standard library doesn't provide a public constructor for it. The current implementation ignores the `code` parameter and returns a zeroed (successful) status.

### Suggestion
Prefix the parameter with an underscore (`_code`) to indicate it is intentionally unused for now.

### Decision

Prefix the parameter with an underscore (`_code`).

We attempted to implement an `ExitStatusExt` trait to allow creating an `ExitStatus` from a raw integer (using unsafe pointer casting). However, on the `wasm32-wasip1` target, `std::process::ExitStatus` appears to be a Zero-Sized Type (0 bytes), meaning it cannot physically store an exit code. Therefore, we are forced to ignore the code and return a dummy status.

## Dead code in `wasm_interactive.rs`

**File:** [src/wasm_interactive.rs](../src/wasm_interactive.rs)

### Warning
```text
warning: variant `Esc` is never constructed
  --> src/wasm_interactive.rs:16:5
   |
 8 | enum Event {
   |      ----- variant in this enum
...
16 |     Esc,
   |     ^^^

warning: method `clear` is never used
  --> src/wasm_interactive.rs:88:8
   |
34 | impl LineBuffer {
   | --------------- method in this implementation
...
88 |     fn clear(&mut self) {
   |        ^^^^^
```

### Analysis
The `Event::Esc` variant and `LineBuffer::clear` method are defined but never used in the current interactive shell implementation.

### Suggestion
If these are intended for future use (e.g., handling the Escape key or clearing the line), they can be marked with `#[allow(dead_code)]`. Otherwise, they can be removed.

## Unused async callback test

**File:** [src/async_test.rs](../src/async_test.rs#L15)

### Warning
```text
warning: function `test_async_callback` is never used
  --> src/async_test.rs:15:8
   |
15 | pub fn test_async_callback() -> i32 {
   |        ^^^^^^^^^^^^^^^^^^^
```

### Analysis
The function `test_async_callback` is defined but never called. This appears to be a leftover from experimental work or a manual test case used to verify the async interop between Rust/WASM and the JavaScript host.

As `washm` relies heavily on async execution (especially for the shell loop and host calls), having isolated test functions like this is common during development. However, since it's not part of the active codebase or the test suite, it triggers a warning.

## Unused Command trait and Dispatcher in `command.rs`

**File:** [src/command.rs](../src/command.rs#L4)

### Warning
```text
warning: trait `Command` is never used
 --> src/command.rs:4:11
...
warning: struct `Dispatcher` is never constructed
  --> src/command.rs:12:12
```

### Analysis
The `Command` trait and `Dispatcher` struct, along with its associated methods (`new`, `register`, `dispatch`), are entirely unused. This suggests a planned command dispatch system that has been superseded by another mechanism or is yet to be integrated.

In a shell, a dispatcher is typically responsible for looking up and executing commands. In `washm`, command execution might be handled directly by `brush` or through the `coreutils_dispatch.rs` logic, making this generic `Dispatcher` redundant in the current architecture.

## Unused Job management structures in `jobs.rs`

**File:** [src/jobs.rs](../src/jobs.rs#L5)

### Warning
```text
warning: type alias `JobId` is never used
...
warning: struct `Job` is never constructed
...
warning: struct `JobManager` is never constructed
```

### Analysis
As previously researched, `src/jobs.rs` contains a `JobManager` implementation that is a duplicate of functionality provided by `brush-core`. Historical analysis shows this file was part of "Phase 2" of development, but the project has since shifted to using the `JobManager` provided by the vendored `brush-core` crate.

### Suggestion
Since the entire file `src/jobs.rs` is currently unused by the rest of the project (though it is included as a module in `lib.rs`), the cleanest fix is to remove the unused code. If the `JobManager` in this file is intended to eventually replace or augment the one in `brush-core`, it should be kept but marked with `#[allow(dead_code)]`.

## Shared reference to mutable static in `host.rs`

**File:** [src/bridges/host.rs](../src/bridges/host.rs#L38)

### Warning
```text
warning: creating a shared reference to mutable static
  --> src/bridges/host.rs:38:12
   |
38 |         if CACHE.is_none() {
   |            ^^^^^^^^^^^^^^^ shared reference to mutable static
```

### Analysis
This warning highlights a pattern that is being phased out in Rust 2024: taking a shared reference to a `static mut` variable. This is inherently unsafe because there are no guarantees against concurrent access or mutation, which can lead to undefined behavior.

In [src/bridges/host.rs](../src/bridges/host.rs), the `CACHE` is used to store host-provided information (like username and hostname) to avoid repeated expensive calls across the WASM boundary.

### Suggestion
Replace `static mut CACHE` with `std::sync::OnceLock<StartupJson>`. This provides a thread-safe and modern way to handle global state and is the standard replacement for this pattern in modern Rust.

## Unexpected `cfg` condition values in `brush` crates

**Files:** Various files in `brush-core`, `brush-parser`, `brush-interactive`, `brush-shell`.

### Warning
```text
warning: unexpected `cfg` condition value: `serde`
  --> /home/mihailik/washm/target/vendor/brush-core/src/openfiles.rs:27:7
   |
27 | #[cfg(feature = "serde")]
   |       ^^^^^^^^^^^^^^^^^
```

### Analysis
Similar to the `uucore` warnings, these are triggered because the vendored `brush` crates are included as modules, and their features are not declared in `washm`'s `Cargo.toml`. The compiler validates all `#[cfg(feature = "...")]` attributes against the features defined in the root crate.

### Suggestion
Add the missing features to `washm`'s `Cargo.toml`. These include:
- `serde`
- `arbitrary`
- `diagnostics`
- `debug-tracing`
- `completion`
- `experimental`
- `experimental-load`
- `experimental-builtins`

## Future-incompatibility and lints in vendored dependencies

**Files:** Various in `brush-core`, `brush-interactive`, `brush-builtins`

**Warnings:**
- `#[path]` attribute cannot be used on type aliases (Future Error)
- `unused attribute #[macro_export]`
- `variable does not need to be mutable`
- `method try_clone is never used`
- `use of async fn in public traits is discouraged`

### Analysis
These warnings occur within the vendored `brush` crates. They range from minor lints (unused methods, unnecessary mutability) to more serious future-incompatibility warnings (the `#[path]` attribute issue).

The `brush` shell is the core engine of `washm`. Since these crates are vendored, we have the ability to patch them, but ideally, these should be addressed in the upstream `brush` repository. The `async fn` in traits warning is particularly relevant as `brush` uses async heavily for its interactive features, and this lint suggests a more robust way to define those traits for future Rust versions.

