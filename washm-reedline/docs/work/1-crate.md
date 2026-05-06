# reedline-async — Path to Publish

Spec for shipping `reedline-async 0.1.0` to crates.io. Companion to
[0-async.md](0-async.md), which contains the architectural rationale.
This document covers only what is left between the current HEAD and a
green publish.

## 0. Verified state at HEAD `be21b2a`

Confirmed by reading the tree on 2026-04-29:

- `Cargo.toml`: `name = "reedline-async"`, `version = "0.1.0"`,
  `[lib] name = "reedline"`. Rename is done; downstream `use reedline::…`
  imports continue to work.
- `cargo check` (default features): passes.
- `cargo check --target wasm32-unknown-unknown --no-default-features`: passes.
- `cargo test --lib --no-run`: **fails to compile**, 15 errors. See § 2.
- Async traits land: `Completer`, `Hinter`, `Highlighter`, `Validator`,
  `History`, `Menu` all return `Pin<Box<dyn Future<…>>>`.
- `Painter<W>` is generic over `W: Write`. `TerminalControl` trait
  abstracts crossterm.
- Feature `crossterm-backend` (default) gates `crossterm` and `fd-lock`.
- 19 examples in `examples/` plus one sub-crate `examples/wasm-repl-demo/`.

Anything not listed here was not verified for this spec.

## 1. Repository hygiene (must precede publish)

These are committed to the working tree and will ship in the `.crate`
tarball unless removed or excluded.

| Path | Action | Reason |
|---|---|---|
| `engine_branch.rs` | delete | leftover scratch file at repo root, not in `src/` |
| `event_source_branch.rs` | delete | same |
| `external_printer_branch.rs` | same | same |
| `temp_columnar.txt` | delete | scratch dump |
| `dummy_crossterm/` | exclude from publish | path-patch source for `examples/wasm-repl-demo` only |
| `dummy_fdlock/` | exclude from publish | same |
| `examples/wasm-repl-demo/` | exclude from publish | nested crate; not part of the library |
| `docs/`, `target/`, `.playwright-mcp/` | exclude from publish | not library content |

Implementation: add an explicit `exclude` (or `include`) list to
`[package]` in `Cargo.toml`, then run `cargo package --list` and verify
nothing in the table above appears.

```toml
[package]
exclude = [
    "engine_branch.rs",
    "event_source_branch.rs",
    "external_printer_branch.rs",
    "temp_columnar.txt",
    "dummy_crossterm/",
    "dummy_fdlock/",
    "examples/wasm-repl-demo/",
    "docs/",
    ".playwright-mcp/",
    "UX_TESTING.md",
]
```

## 2. Test compilation (blocker for `cargo test --lib`)

`cargo test --lib --no-run` reports 15 errors, all caused by call sites
that consume `Pin<Box<dyn Future<…>>>` as if it were the unwrapped
value. Each must be wrapped in `futures::executor::block_on(...)` (or
`pollster::block_on` after § 4).

Concrete sites:

- [src/engine.rs](src/engine.rs)
  - L100 — `E0283` type annotations
  - L2099 — `.expect(...)` on `Pin<Box<dyn Future<Output = Result<HistoryItem, _>>>>`
  - L2182 — mismatched types
  - L2188 — type annotations
- [src/completion/default.rs](src/completion/default.rs)
  - L430 — mismatched types
- [src/completion/history.rs](src/completion/history.rs)
  - L117, L125, L127, L130, L131, L132, L138, L156, L161 — eight occurrences,
    all of the form `completer.complete(...)` indexed/iterated as if it
    were `Vec<Suggestion>`. Wrap each call in `block_on`.
- [src/menu/ide_menu.rs](src/menu/ide_menu.rs)
  - L1243 — `complete` trait signature mismatch.

Search command (no look-around; rg uses Rust regex):

```pwsh
rg -n '\.(complete|update_values|can_partially_complete|update_working_details|save|sync|load|load_at_id)\(' src/ |
  rg -v 'block_on|fn '
```

Acceptance: `cargo test --lib --no-run` exits 0; subsequent
`cargo test --lib` reports the menu unicode tests as the only failures
(see § 3).

## 3. Runtime test failures

After § 2 compiles cleanly, the only known runtime regressions are
three unicode tests in [src/menu/columnar_menu.rs](src/menu/columnar_menu.rs):

- L859 `test_menu_create_string`
- L871 `test_menu_create_string_starting_with_multibyte_char`
- L883 `test_menu_create_string_long_unicode_string`

The handover memory file describes the source as mojibake. A byte-level
read of the file at HEAD shows the multibyte literals are intact UTF-8.
**Verify before acting**:

```pwsh
Format-Hex src/menu/columnar_menu.rs -Count 64 -Offset 28100
git show be21b2a:src/menu/columnar_menu.rs | Select-String 'おは|验'
```

If the literals are correct in the file but the tests still fail, the
real cause is § 4: `update_working_details` does not call
`update_values`, so the menu is laid out against an empty `values`
vector and `menu_string(...)` panics or returns "".

Last commit that touched this file pre-async work: `0080077` ("Strip
ANSI before computing suggestion width"). Use it as a recovery point
only if a byte check actually shows mojibake.

## 4. Menu fix: `update_working_details` must populate values

In the original synchronous code, `update_working_details` indirectly
called `update_values`. The async stubs in
[src/menu/columnar_menu.rs](src/menu/columnar_menu.rs) and
[src/menu/ide_menu.rs](src/menu/ide_menu.rs) skip that call, so
`self.values` stays empty and downstream layout subtracts from 0.

Fix:

```rust
fn update_working_details<'a, W: Write>(
    &'a mut self,
    editor: &'a mut Editor,
    completer: &'a mut dyn Completer,
    painter: &'a Painter<W>,
) -> super::MenuFuture<'a, ()> {
    Box::pin(async move {
        self.update_values(editor, completer).await;
        // existing event-handling body
    })
}
```

Apply to both menus.

## 5. `FileBackedHistory::Drop` is currently a no-op

[src/history/file_backed.rs](src/history/file_backed.rs#L376-L380):

```rust
impl Drop for FileBackedHistory {
    fn drop(&mut self) {
        let _res = self.sync();
    }
}
```

`sync()` now returns `Pin<Box<dyn Future<Output = io::Result<()>>>>`
([src/history/file_backed.rs](src/history/file_backed.rs#L240)). The
returned future is dropped, never polled. **Nothing is flushed to
disk.** Tests that read history back from a file see an empty file.

Fix:

```rust
impl Drop for FileBackedHistory {
    fn drop(&mut self) {
        let _ = futures::executor::block_on(self.sync());
    }
}
```

Footgun to document in [CONTRIBUTING.md](CONTRIBUTING.md) or rustdoc:
calling `drop` on a `FileBackedHistory` that lives inside a Tokio
multi-thread runtime can deadlock the current worker. Out of scope for
0.1.0; track as a follow-up (see § 11).

## 6. Test-side executor

Once § 2 is done, normalise the `block_on` choice in tests:

- Add `pollster = "0.3"` to `[dev-dependencies]`.
- In `#[cfg(test)]` modules, use `pollster::block_on`.
- Keep `futures::executor::block_on` only where required at runtime
  (`Drop` impls; § 5).

The motivation is a smaller dev-dependency footprint; both executors
have the same waker semantics, so do not advertise behavioural
differences.

## 7. Examples

Nineteen example binaries live under [examples/](examples). All call
into a now-async `read_line`. Wrap each `main` in a single
`pollster::block_on(...)` call:

```rust
fn main() -> io::Result<()> {
    pollster::block_on(async_main())
}

async fn async_main() -> io::Result<()> { /* existing body */ }
```

Files to update: `basic.rs`, `completions.rs`, `custom_prompt.rs`,
`cwd_aware_hinter.rs`, `demo.rs`, `event_listener.rs`,
`event_listener_kitty_proto.rs`, `helix.rs`, `highlighter.rs`,
`hinter.rs`, `history.rs`, `ide_completions.rs`, `list_bindings.rs`,
`mouse_click.rs`, `platform_agnostic_wasm.rs`, `transient_prompt.rs`,
`validator.rs`, `semantic_prompt_interactive.rs`,
`semantic_prompt_verify.rs`.

Add `pollster` to `[dev-dependencies]` (already done in § 6); examples
inherit dev-deps.

`examples/wasm-repl-demo/` is a separate crate with its own
`Cargo.toml`. Leave it untouched for now; document it as a separate
build target in its `README`.

## 8. Documentation

- [README.md](README.md): replace the synchronous quickstart with the
  async one. Verify the doctest builds with `cargo test --doc`.
- [src/lib.rs](src/lib.rs): same for the crate-level doctest.
- New file `MIGRATING.md` at repo root, scope: a concise diff from
  upstream `reedline 0.47` to `reedline-async 0.1.0`. List
  trait-signature changes, removed sync `read_line`, removed
  `ExternalPrinter`, the `crossterm-backend` feature flag.
  This is the only new markdown file the spec authorises.

## 9. CI

Update `.github/workflows/` to:

- Build matrix:
  - `cargo test --lib` on `ubuntu-latest`, `macos-latest`, `windows-latest`.
  - `cargo test --doc`.
  - `cargo check --no-default-features` on `ubuntu-latest`.
  - `cargo check --target wasm32-unknown-unknown --no-default-features`
    on `ubuntu-latest` (install via `rustup target add`).
  - Feature combinations: `bashisms`, `helix`, `sqlite`,
    `system_clipboard`, all-features.
- `cargo doc --no-deps` builds clean (warnings as errors).
- `cargo package --list` runs as a publish-dryrun gate; fail if any
  path from § 1 appears.

## 10. Publish

1. `cargo package --list` — manually inspect output against § 1.
2. `cargo publish --dry-run`.
3. Tag `v0.1.0` and push.
4. `cargo publish`.
5. Open a tracking issue for v0.2.x covering § 11.

## 11. Out of scope for 0.1.0

Track as separate issues:

1. `Reedline::print_above_prompt` async API to replace the deleted
   `ExternalPrinter`.
2. `MemoryEventSource` and `MemoryHistory` for WASM hosts.
3. WASM-compatible `Painter` writer (canvas / xterm.js bindings).
4. Async `open_editor` (currently `child.wait()`, native-only).
5. RPITIT migration of trait methods; will require MSRV bump to 1.75+.
6. `Reedline::step()` low-level single-event API.
7. `FileBackedHistory::drop` deadlock guard under Tokio runtimes (§ 5).

## 12. Ordering and dependencies

```
§1  hygiene  ──┐
               ├─►  §2 test compile  ──► §4 menu fix  ──► §3 unicode tests pass
§5  Drop fix ──┘                                          │
                                                          ▼
                                              §6 executor ► §7 examples ► §8 docs ► §9 CI ► §10 publish
```

§ 1, § 2, § 4, § 5 are independent and can land in any order. § 3 is a
verification step, not work. Everything from § 6 onward is gated on a
green `cargo test --lib`.

## 13. Working rules for contributors

1. Edit `.rs` files only through the editor or `replace_string_in_file`
   tooling. Do **not** round-trip Rust source through PowerShell
   `Get-Content`/`Set-Content`; the default Windows encoding is
   UTF-16 LE and silently corrupts multibyte literals. Use
   `-Encoding utf8` if PowerShell is unavoidable.
2. Every async call in test code must be wrapped in `block_on`. An
   unawaited future compiles silently and surfaces as
   "subtract with overflow" inside the menu layout.
3. Never `#[ignore]` or delete a failing test to green CI without a
   linked tracking issue. If you must skip, cite this document.
4. Before claiming a runtime fix, run the specific test:
   `cargo test --lib path::to::test -- --nocapture`.
5. `cargo test --lib --no-run` first, then `cargo test --lib`. Compile
   errors are cheaper to read than runtime panics.
