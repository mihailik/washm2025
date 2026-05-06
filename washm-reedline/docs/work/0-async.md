# Reedline Async Migration Report

## Executive Summary

This report analyses the reedline codebase (v0.47.0) to assess the work required to move all
input handling to an asynchronous model — one where reading from stdin does not block a dedicated
thread and where the library can be used on async runtimes (Tokio, async-std), in WASM environments,
and on any platform that exposes callback-based or non-blocking stdin consumption.

The conclusion is that the migration is **substantial but tractable**. The core editing logic
(`Editor`, `LineBuffer`, `EditCommand`, all trait impls) is already `Send` and free of I/O; the
entire blocking surface is confined to a handful of call-sites inside `engine.rs` and the
`crossterm` dependency used there. Platform shims, public API changes, and optional async-runtime
support can be layered incrementally without discarding the existing synchronous interface.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Where Blocking Happens Today](#where-blocking-happens-today)
3. [Trait Audit — What Must Change](#trait-audit--what-must-change)
4. [The Event Pipeline in Detail](#the-event-pipeline-in-detail)
5. [Platform Considerations](#platform-considerations)
6. [Proposed Migration Strategy](#proposed-migration-strategy)
7. [Dependency Changes](#dependency-changes)
8. [Effort Estimate by Layer](#effort-estimate-by-layer)
9. [Open Design Questions](#open-design-questions)

---

## Architecture Overview (Updated)

Reedline's async architecture is now decoupled from specific I/O backends, enabling WASM support and non-blocking operation:

```
┌─────────────────────────────────────────────────────────┐
│  Public API  (Reedline, Signal, read_line_async)        │
├─────────────────────────────────────────────────────────┤
│  Engine  (engine.rs) — Now Generic over W: Write        │
│   ├── Event loop: .next_event().await                   │
│   ├── TerminalControl: Abstraction for Raw Mode/Size    │
│   └── Painter<W>: Decoupled from Stderr                 │
├────────────────┬──────────────────┬─────────────────────┤
│  Editor        │  Painter<W>      │  Async History      │
│  (core_editor) │  (painting/)     │  (history/)         │
│  Pure Rust     │  Generic Output  │  Async traits       │
│  no I/O        │  TerminalControl │                     │
├────────────────┴──────────────────┴─────────────────────┤
│  Async Plug-in traits: Completer, Hinter, Highlighter,  │
│  Validator, Menu, Prompt                                │
└─────────────────────────────────────────────────────────┘
```

### Key Innovations Added

1. **TerminalControl Trait**: A platform-agnostic control plane for terminal operations (raw mode, cursor visibility, size queries). This allows `Painter` and `Engine` to function without direct `crossterm` ties.
2. **Generic Painter**: `Painter<W>` supports any `std::io::Write + Send`, making it compatible with `Vec<u8>` for testing or custom buffers for WASM.
3. **Async Menu Trait**: Fully async menu interaction using `Pin<Box<dyn Future>>` to handle complex lifetimes without external dependencies.
4. **External Printer (Planned)**: To be ported from historical branches to allow non-blocking log injection via channels.

---

## Where Blocking Happens Today (Resolved)

### 1. The core event loop — `engine.rs:read_line_helper`

This is the **single most important blocking site**. The entire method runs on the calling thread
and never yields:

```rust
// src/engine.rs  lines 808-960
loop {
    // ...
    if needs_polling {
        if event::poll(self.poll_interval)? {          // ← blocking syscall (epoll/kqueue/IOCP)
            events.push(crossterm::event::read()?);    // ← blocking read
        }
    } else {
        events.push(crossterm::event::read()?);        // ← unconditionally blocking
    }

    // drain remaining events without blocking
    while !completed(&events) && event::poll(Duration::from_millis(0))? {
        events.push(crossterm::event::read()?);
    }

    // paste / resize batching
    if events.len() > EVENTS_THRESHOLD || ... {
        while !completed(&events) && event::poll(POLL_WAIT)? {
            events.push(crossterm::event::read()?);    // ← blocking with 100 ms timeout
        }
    }

    // handle events, repaint ...
}
```

`crossterm::event::poll` and `crossterm::event::read` are wrappers around:
- **Linux/macOS**: `epoll_wait` / `kevent` (via `mio`)
- **Windows**: `WaitForMultipleObjects` / `ReadConsoleInput`
- **WASM**: not supported at all — `crossterm` panics or fails to compile

Neither call is `async`; both occupy the thread for the full wait duration.

### 2. The `idle_callback` workaround — `engine.rs:811-813`

```rust
#[cfg(feature = "idle_callback")]
if let Some(ref mut callback) = self.idle_callback {
    callback();  // ← called synchronously at the top of every iteration
}
```

This is the existing mechanism for applications that need periodic processing while the editor
is alive. It works, but it requires the caller to pack all their async work into a synchronous
callback and deal with the 100 ms poll interval themselves — not a real async solution.

### 3. The `external_printer` workaround — `engine.rs:826-839`

```rust
#[cfg(feature = "external_printer")]
if let Some(ref external_printer) = self.external_printer {
    let messages = Self::external_messages(external_printer)?;
    // ...
}
```

`ExternalPrinter` uses a `crossbeam` bounded channel (`src/external_printer.rs`):

```rust
pub struct ExternalPrinter<T: Display> {
    sender: Sender<T>,
    receiver: Receiver<T>,
}
```

Another thread sends messages; the engine drains them with `try_recv()` (non-blocking) once per
poll iteration. This is a minimal bridge between synchronous reedline and an async-friendly world,
but it requires spawning a thread and cannot work in single-threaded environments (WASM).

### 4. The `break_signal` workaround — `engine.rs:815-823`

```rust
if let Some(ref signal) = self.break_signal {
    if signal.swap(false, std::sync::atomic::Ordering::Relaxed) {
        // return Signal::ExternalBreak
    }
}
```

An `Arc<AtomicBool>` that another thread can set to interrupt the loop. Thread-safe, but again
requires a second thread and does not translate to async.

### 5. Terminal setup/teardown — `engine.rs:752-761`

```rust
pub fn read_line(&mut self, prompt: &dyn Prompt) -> Result<Signal> {
    terminal::enable_raw_mode()?;    // ← sync OS call
    self.bracketed_paste.enter();    // ← write escape sequence to stdout
    self.kitty_protocol.enter();     // ← write escape sequence to stdout
    let result = self.read_line_helper(prompt);
    self.bracketed_paste.exit();
    self.kitty_protocol.exit();
    terminal::disable_raw_mode()?;
    result
}
```

The terminal mode changes themselves are fast, but they are synchronous writes. In an async
context they need to be wrapped or moved to a dedicated blocking-safe context.

### 6. `open_editor` — `engine.rs:1880-1903`

```rust
fn open_editor(&mut self) -> Result<()> {
    let mut child = command.spawn()?;
    child.wait()?;       // ← blocks the thread until the editor exits
    // ...
}
```

`std::process::Command::wait()` blocks indefinitely. In async code this must be replaced with
`tokio::process::Command` (or equivalent).

### 7. History I/O — `history/base.rs`

The `History` trait is sync:

```rust
pub trait History: Send {
    fn save(&mut self, h: HistoryItem) -> Result<HistoryItem>;
    fn load(&self, id: HistoryItemId) -> Result<HistoryItem>;
    fn search(&self, query: SearchQuery) -> Result<Vec<HistoryItem>>;
    fn sync(&mut self) -> std::io::Result<()>;
    // ...
}
```

`FileBackedHistory::sync()` calls `fs::write` (blocking). `SqliteBackedHistory` calls `rusqlite`
which in turn uses blocking POSIX file I/O. These are called from the event loop body during
history traversal (`previous_history`, `next_history`, `history_cursor.back/forward`).

In practice history queries are very fast (milliseconds), but they block the calling thread and
prevent proper async usage. In a WASM environment there is no blocking file I/O at all.

### 8. `Painter` writes — `painting/painter.rs`

```rust
pub type W = std::io::BufWriter<std::io::Stderr>;

pub struct Painter {
    stdout: W,
    // ...
}
```

All terminal painting calls queue commands then flush via `self.stdout.flush()`. This is blocking.
For async use these writes need either:
- an async `Write` implementation (e.g., `tokio::io::BufWriter<AsyncStderr>`), or
- to be dispatched to a blocking thread pool.

---

## Trait Audit — What Must Change

### Traits that are **already async-compatible** (no changes required for pure logic)

| Trait | Location | Why already OK |
|---|---|---|
| `EditMode` | `edit_mode/base.rs` | Pure translation; no I/O |
| `Highlighter` | `highlighter/mod.rs` | Pure text transformation |
| `Validator` | `validator/mod.rs` | Pure text validation |
| `Completer` | `completion/base.rs` | May be slow but takes `&mut self`; easily wrapped |
| `Prompt` | `prompt/base.rs` | Returns `Cow<'_, str>`; pure |
| `EditCommand` | `enums.rs` | Data enum; no I/O |
| `ReedlineEvent` | `enums.rs` | Data enum; no I/O |
| `Editor` | `core_editor/editor.rs` | Pure in-memory edits |
| `LineBuffer` | `core_editor/line_buffer.rs` | Pure buffer operations |

### Traits/types that **must gain async variants**

| Trait | Issue | Suggested async form |
|---|---|---|
| `History` | Blocking file/DB I/O | `AsyncHistory: Send + Sync` with `async fn save/load/search/sync` |
| `Hinter` | Calls `History` synchronously | Mirror to `AsyncHinter` or make it accept `&dyn AsyncHistory` |
| `ExternalPrinter` | `crossbeam` channel (thread-only) | Replace with a `tokio::sync::mpsc` channel |

### The `Reedline` struct and `read_line`

The main change: introduce an async entry point alongside the existing sync one.

```rust
// Current sync API (keep for backward compat)
pub fn read_line(&mut self, prompt: &dyn Prompt) -> std::io::Result<Signal>;

// New async API
pub async fn read_line_async(&mut self, prompt: &dyn Prompt) -> std::io::Result<Signal>;
```

---

## The Event Pipeline in Detail

Understanding the exact data flow is essential for an async redesign:

```
Keyboard/Mouse
     │
     ▼
crossterm::event::read()         ← blocking OS call (mio)
     │  returns Event (crossterm type)
     ▼
ReedlineRawEvent::try_from(event) ← filters KeyRelease, normalises Repeat→Press
     │  returns ReedlineRawEvent (wrapper)
     ▼
EditMode::parse_event(event)      ← keybinding table lookup
     │  returns ReedlineEvent
     ▼
Reedline::handle_event()          ← dispatches based on InputMode
     ├── handle_history_search_event()
     └── handle_editor_event()
          ├── run_edit_commands()  →  Editor::run_edit_command()  ← pure
          ├── history_cursor ops  →  History::search()           ← blocking I/O
          ├── submit_buffer()     →  History::save()             ← blocking I/O
          ├── open_editor()       →  Command::wait()             ← blocking process
          └── return EventStatus::{Handled, Exits, Inapplicable}
     │
     ▼
Reedline::repaint()               ← Painter::repaint_buffer()    ← blocking write
```

In an async version this pipeline needs to become a `Future`-based stream:

```
AsyncEventSource (platform-specific)
     │  yields Event items asynchronously (poll_read / callback / WASM event)
     ▼
same ReedlineRawEvent normalisation (no change needed — pure)
     ▼
same EditMode::parse_event        (no change needed — pure)
     ▼
same handle_event logic           (no change needed — pure logic)
     │
     ├── async History queries    ← await on spawn_blocking or async DB
     └── async open_editor        ← tokio::process or spawn_blocking
     │
     ▼
async Painter write               ← AsyncWrite or spawn_blocking
```

---

## Platform Considerations

### Unix (Linux, macOS)

crossterm already uses `mio` on Unix, which wraps `epoll`/`kqueue`. Tokio also uses `mio`.
**The path of least resistance**: register the terminal file descriptor with Tokio's reactor and
read events from within an `async fn`. The `crossterm` crate does not yet expose an async API, but:

- `crossterm::event::EventStream` exists in crossterm's `event-stream` feature and provides
  an `async fn next()` backed by Tokio or async-std:
  ```toml
  crossterm = { version = "0.29", features = ["event-stream"] }
  ```
  ```rust
  use crossterm::event::EventStream;
  use futures::StreamExt;

  let mut reader = EventStream::new();
  while let Some(event) = reader.next().await {
      // event: crossterm::Result<Event>
  }
  ```
  This is the **lowest-effort path on Unix/Windows** — adding one feature flag to crossterm gives
  a fully non-blocking event stream with no new dependencies.

### Windows

`crossterm` on Windows uses `WaitForMultipleObjects` via `crossterm_winapi`. The `event-stream`
feature wraps this in an async task that calls `WaitForMultipleObjects` with a timeout on a
blocking thread and sends results to a channel — effectively `spawn_blocking` under the hood.
This is acceptable for a first version. Full async Windows console I/O would require
`ReadFileEx` with an IOCP completion port, which crossterm does not expose. The channel approach
has near-zero overhead in practice.

### WASM

`crossterm` does not compile to WASM; it will either fail to build or provide stub
implementations. For WASM reedline must:

1. Receive keyboard events via a **JavaScript callback** (e.g., `keydown` event on a DOM element
   or a xterm.js `onKey` handler).
2. Convert those DOM events to the `ReedlineRawEvent` type used internally.
3. Drive the `handle_event` state machine from that callback (single-threaded, no blocking allowed).

Since `crossterm::event::Event` is a concrete Rust enum, the conversion from DOM events to
`ReedlineRawEvent` is straightforward but requires a new `wasm32` back-end module.

The `Painter` also does not work on WASM because it writes ANSI escape sequences to stderr.
A WASM-compatible painter would need to render to a string or a canvas — out of scope for a
minimal async migration but architecturally the same challenge.

```
WASM approach:

[DOM keydown event]
        │  (wasm-bindgen closure)
        ▼
[ReedlineRawEvent::from_dom_key_event()]    ← new conversion fn
        │
        ▼
[existing EditMode::parse_event()]          ← unchanged
        │
        ▼
[existing handle_event()]                   ← unchanged (pure)
        │
        ▼
[WASM-compatible output writer]             ← new: write styled strings to xterm.js / canvas
```

---

## Proposed Migration Strategy

### Phase 1 — Abstract the Event Source (Low Risk, High Value)

Introduce a trait that represents "something that produces terminal events":

```rust
// New file: src/event_source.rs

/// Produces raw crossterm events, either synchronously or asynchronously.
pub trait EventSource: Send {
    /// Block until the next event is available (used by the sync read_line).
    fn next_event_sync(&mut self) -> std::io::Result<crossterm::event::Event>;

    /// Poll without blocking; returns `None` immediately if no event is ready.
    fn try_next_event(&mut self) -> std::io::Result<Option<crossterm::event::Event>>;
}

/// The default implementation using crossterm's blocking API (unchanged behaviour).
pub struct CrosstermEventSource;

impl EventSource for CrosstermEventSource {
    fn next_event_sync(&mut self) -> std::io::Result<crossterm::event::Event> {
        crossterm::event::read()
    }
    fn try_next_event(&mut self) -> std::io::Result<Option<crossterm::event::Event>> {
        if crossterm::event::poll(Duration::ZERO)? {
            Ok(Some(crossterm::event::read()?))
        } else {
            Ok(None)
        }
    }
}
```

Add `event_source: Box<dyn EventSource>` to `Reedline`. The existing `read_line_helper`
replaces its direct `crossterm::event::poll/read` calls with `self.event_source.next_event_sync()`
and `self.event_source.try_next_event()`. **No behavioural change for existing users.**

### Phase 2 — Add Async Event Source Variant

Extend the trait with an async method behind a feature flag:

```rust
// Feature: async (adds tokio dependency)

#[cfg(feature = "async")]
pub trait AsyncEventSource: Send {
    fn next_event<'a>(
        &'a mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<crossterm::event::Event>> + Send + 'a>>;
}

#[cfg(feature = "async")]
pub struct CrosstermAsyncEventSource {
    stream: crossterm::event::EventStream,
}

#[cfg(feature = "async")]
impl AsyncEventSource for CrosstermAsyncEventSource {
    fn next_event<'a>(
        &'a mut self,
    ) -> std::pin::Pin<Box<dyn Future<Output = std::io::Result<Event>> + Send + 'a>> {
        Box::pin(async move {
            use futures::StreamExt;
            self.stream
                .next()
                .await
                .unwrap_or(Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)))
        })
    }
}
```

### Phase 3 — Async `read_line`

Add a new method to `Reedline` gated on the `async` feature:

```rust
#[cfg(feature = "async")]
impl Reedline {
    pub async fn read_line_async(&mut self, prompt: &dyn Prompt) -> std::io::Result<Signal> {
        crossterm::terminal::enable_raw_mode()?;
        self.bracketed_paste.enter();
        self.kitty_protocol.enter();

        let result = self.read_line_helper_async(prompt).await;

        self.bracketed_paste.exit();
        self.kitty_protocol.exit();
        crossterm::terminal::disable_raw_mode()?;
        result
    }

    async fn read_line_helper_async(&mut self, prompt: &dyn Prompt) -> std::io::Result<Signal> {
        self.painter.initialize_prompt_position(self.suspended_state.as_ref())?;
        // ... same init logic ...
        self.repaint(prompt)?;   // painting stays sync for now (Phase 4)

        loop {
            // 1. Check break signal / external printer (unchanged logic)
            // ...

            // 2. Await next event — no thread blocked!
            let event = self.async_event_source.next_event().await?;

            // 3. Convert and dispatch — all pure logic, no awaits needed
            if let Ok(raw) = ReedlineRawEvent::try_from(event) {
                let reedline_event = self.edit_mode.parse_event(raw);
                match self.handle_event(prompt, reedline_event)? {
                    EventStatus::Exits(signal) => {
                        if self.suspended_state.is_none() {
                            self.painter.move_cursor_to_end()?;
                        }
                        return Ok(signal);
                    }
                    EventStatus::Handled => self.repaint(prompt)?,
                    EventStatus::Inapplicable => {}
                }
            }
        }
    }
}
```

Key observation: **most of `handle_event` and all of `run_edit_commands` are already pure
synchronous Rust with no I/O** — they just mutate in-memory state. Only two paths inside
`handle_editor_event` call I/O:

- `history_cursor.back/forward` → `History::search` (very fast, can stay sync or be
  wrapped with `spawn_blocking`)
- `open_editor` → `Command::wait` (must become `async` or `spawn_blocking`)

### Phase 4 — Async Painter (Optional, Required for WASM)

The `Painter` writes to `BufWriter<Stderr>`. For a non-WASM async version this is fine —
`std::io::Write` on stderr rarely blocks in practice and `spawn_blocking` is overkill. For
completeness:

```rust
// Parameterize Painter over the writer type
pub struct Painter<W: std::io::Write> {
    stdout: W,
    // ...
}

// Existing default
pub type DefaultPainter = Painter<std::io::BufWriter<std::io::Stderr>>;
```

For WASM the writer could be a `Vec<u8>` or a closure that calls into JavaScript.

### Phase 5 — Async History (Optional)

For WASM environments that cannot do blocking I/O, the `History` trait needs an async version:

```rust
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncHistory: Send {
    async fn save(&mut self, h: HistoryItem) -> crate::Result<HistoryItem>;
    async fn search(&self, query: SearchQuery) -> crate::Result<Vec<HistoryItem>>;
    async fn sync(&mut self) -> std::io::Result<()>;
    // ...
}
```

On native platforms `FileBackedHistory` can implement `AsyncHistory` by delegating to
`tokio::task::spawn_blocking`. On WASM a new `LocalStorageHistory` or `IndexedDbHistory`
backend can be written that uses the browser's asynchronous storage APIs.

### Phase 6 — WASM & Platform Abstraction (In Progress)

This phase focuses on the "Final Decoupling" to enable `wasm32-unknown-unknown` targets.

1. **`EventSource` Trait**:
    - *Goal*: Abstract `crossterm::event` away from the engine.
    - *Native*: Use `crossterm::event::EventStream`.
    - *WASM*: Use a channel driven by JS `keydown` listeners.
2. **`ExternalPrinter` Integration**:
    - *Borrowed from*: `copilot/async-file-implementation-164d91a0`.
    - *Benefit*: Thread-safe and task-safe line injection. Uses `futures::mpsc` instead of `crossbeam`.
3. **`RenderSnapshot` (Experimental)**:
    - *Research*: Borrowed from `investigate-async-input-handling`.
    - *Benefit*: Declarative UI state to minimize terminal flickering in high-concurrency event scenarios.

---

## Status Tracking

| Feature | Status | Implementation Detail |
| :--- | :--- | :--- |
| **Async Engine Loop** | ✅ Done | `read_line` is now `async` |
| **Generic Painter** | ✅ Done | `Painter<W>` implemented |
| **TerminalControl** | ✅ Done | Integrated into Engine/Painter |
| **Async Traits** | ✅ Done | Completer, Hinter, Validator, History |
| **EventSource Trait** | ⏳ Planned | Porting from `async-file-implementation` |
| **External Printer** | ⏳ Planned | Porting from `async-file-implementation` |
| **WASM Support** | 🔄 Research | Evaluating `no-Send` trait variants |

---

## Dependency Changes

| Change | Scope | Rationale |
|---|---|---|
| Add `crossterm` feature `event-stream` | `async` feature | Provides `EventStream` for Unix/Windows async events |
| Add `tokio` (or `async-std`) | `async` feature (optional) | Async runtime for `spawn_blocking`, `process` |
| Add `futures` (or `futures-util`) | `async` feature | `StreamExt`, channel types |
| Add `async-trait` | `async` feature | Ergonomic `async fn` in traits (or use RPITIT in Rust ≥ 1.75) |
| Remove `crossbeam` from `external_printer` | `async` feature only | Replace with `tokio::sync::mpsc` |
| Add `wasm-bindgen` / `wasm-bindgen-futures` | `wasm` feature | JavaScript interop |

The crate currently requires **Rust 1.63**. Async traits without `async-trait` require 1.75
(RPITIT). If the MSRV must stay at 1.63, `async-trait` is necessary for Phase 5.

---

## Effort Estimate by Layer

| Layer | Estimated Effort | Risk |
|---|---|---|
| Phase 1: Abstract EventSource trait | 1–2 days | Low — pure refactor, no behaviour change |
| Phase 2: Async EventSource (crossterm `event-stream`) | 1 day | Low — feature already exists in crossterm |
| Phase 3: `read_line_async` method | 2–3 days | Medium — core loop rewrite, needs testing |
| Phase 4: Parameterized Painter | 1–2 days | Low — mostly mechanical generics |
| Phase 5: `AsyncHistory` trait + `spawn_blocking` impls | 2–3 days | Medium — trait design must be stable |
| Phase 6: WASM back-end | 5–10 days | High — new platform, no existing code, needs JS integration tests |
| Integration & regression tests | 3–5 days | Medium — CI matrix with tokio, async-std, WASM |
| **Total (native async only, phases 1–5)** | **~10–16 days** | |
| **Total (including WASM, all phases)** | **~18–28 days** | |

---

## Open Design Questions

1. **Runtime-agnostic or Tokio-first?**
   Targeting both Tokio and async-std requires either `async-std` compatibility shims or
   abstracting over the executor. The `crossterm` `event-stream` feature already supports both
   via `futures-util`. Using only `futures::Stream` / `futures::Future` in the public API (no
   Tokio types in signatures) keeps the crate runtime-agnostic, but the `open_editor` and
   `AsyncHistory` impls will need runtime-specific `spawn_blocking`.

2. **Backward compatibility of `read_line`?**
   The existing sync `read_line` must remain. It can delegate to the blocking `CrosstermEventSource`
   with no change to users on native platforms. The two code paths can share nearly all logic via
   the `handle_event`/`run_edit_commands` machinery (already pure sync).

3. **Single-threaded WASM constraint.**
   All `Send` bounds on traits (`History: Send`, `Hinter: Send`, etc.) must be reviewed for WASM.
   On WASM `!Send` types can be used because there is only one thread. Either the bounds need to
   be relaxed behind `#[cfg(not(target_arch = "wasm32"))]` or the WASM build must use `Arc<Mutex>`
   wrappers. This is a non-trivial ergonomics question.

4. **`Completer` and `Hinter` latency.**
   In some shells completions are computed asynchronously (network lookups, subprocess calls).
   `Completer` and `Hinter` take `&mut self` synchronously. An async wrapper could be:
   ```rust
   pub trait AsyncCompleter: Send {
       async fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion>;
   }
   ```
   This is a bonus improvement, not required for the minimal async migration.

5. **Paste batching in async context.**
   The current paste-detection heuristic (`EVENTS_THRESHOLD = 10`) uses timed polls:
   ```rust
   while !completed(&events) && event::poll(POLL_WAIT)? { ... }
   ```
   In an async loop this becomes `tokio::time::timeout(POLL_WAIT, stream.next())` — equivalent
   semantics, slightly different code.

6. **`ExternalPrinter` in async context.**
   Replace `crossbeam::channel` with `tokio::sync::mpsc`:
   ```rust
   #[cfg(feature = "async")]
   pub struct AsyncExternalPrinter<T: Display + Send> {
       sender: tokio::sync::mpsc::Sender<T>,
       receiver: tokio::sync::mpsc::Receiver<T>,
   }
   ```
   The engine loop would select over both the event stream and the printer receiver:
   ```rust
   tokio::select! {
       event = event_source.next_event() => { /* handle input */ }
       msg   = printer_rx.recv()         => { /* print external message */ }
   }
   ```
   This is cleaner and more idiomatic than the current timed polling approach.

---

## Summary

The reedline codebase is well-structured for this migration:

- **~95% of the code is already pure, synchronous, and runtime-agnostic.** The `Editor`,
  `EditMode`, `Highlighter`, `Completer`, `Hinter`, `Validator`, `Prompt`, and all the
  `ReedlineEvent`/`EditCommand` machinery have no I/O and need no changes.

- **The entire blocking surface is in six places** inside `engine.rs` (`crossterm::event::read`,
  `event::poll`, `History` calls, `Command::wait`, terminal mode switches, and painter flushes).

- **crossterm already ships the necessary async abstraction** (`EventStream` behind the
  `event-stream` feature) for Unix and Windows. This is the most important dependency change and
  it requires only a single feature flag.

- **The public API can grow an async variant** (`read_line_async`) without removing the
  synchronous `read_line`, preserving backward compatibility completely.

- **WASM support is the hardest part** because it requires replacing crossterm entirely with a
  DOM event adapter and replacing the ANSI-escape `Painter` with a JavaScript-callable output
  writer. This is new work but the internal logic that must be reused is already completely
  decoupled from I/O.

---

# Part II — Deep Dive: Single-Threaded, Tokio-Free, Backward-Compat-Free

> **Updated requirements (revised plan):**
> 1. Must run on a **completely single-threaded** platform (e.g. browser WASM main thread). No worker threads, no parallelism — only cooperative async.
> 2. **No Tokio dependency.** Avoid runtime lock-in entirely. Remove Tokio if it ever sneaks in.
> 3. **No backward compatibility.** The sync `read_line` API and all of its scaffolding (`break_signal`, `idle_callback`, `external_printer` thread channel, `Send` bounds, `Arc<AtomicBool>`, `crossbeam`) are removed outright. The crate becomes async-only.
>
> This part supersedes the layered "phase 1–6 keep-the-sync-API" plan in Part I and replaces it with a clean-slate redesign.

---

## 10. Reframing the Problem

The original plan assumed the goal was *adding* async on top of sync. The revised goal is the
opposite: **the library is async; sync is gone.** That changes several things at the architectural
level:

| Concern | Old plan (Part I) | New plan (Part II) |
|---|---|---|
| Sync `read_line` | Kept for compat | **Removed** |
| `EditMode`/`Highlighter`/`Completer`/`Hinter` | Stay sync | **All become `async fn`** (cooperative yield points) |
| `Send` bounds on traits | Kept for thread-safety | **Removed** — single-threaded |
| `crossbeam`, `Arc<AtomicBool>`, threads | Kept under feature flags | **Removed** entirely |
| `tokio`, `tokio::select!`, `tokio::sync::mpsc` | Used in async path | **Banned**. Use `core::future`, `futures-core`, and a tiny local executor or caller-driven polling |
| Two parallel APIs | Yes | **One async API** |
| `idle_callback`, `break_signal`, `external_printer` features | Kept | **Deleted** — replaced by composing with the caller's own async event sources |
| `crossterm::event::read/poll` | Wrapped in `EventSource` trait | **Eliminated as a runtime dependency** of the engine. Crossterm becomes one optional native back-end among several (CLI, in-process test, WASM/DOM). |

The result is a smaller, simpler, more portable crate.

### Note on point #1: there is no parallelism today

The user's task statement says "highlights and completions currently rely on parallel rather than
async processing." Reading the code carefully (see [§ 11](#11-the-myth-of-parallel-highlightcomplete)),
this is not literally true — the highlighter and completer execute synchronously on the same
thread as the event loop. **What looks like "parallel" is actually "blocking the loop while
running"**, which is even worse for a single-threaded async target: a long-running highlighter
freezes the entire UI with no way to yield. The async migration's job is to add **cooperative
yield points** inside these traits so that on a slow completer, the runtime can poll other tasks
(redraw timer, paste-flush, async stdin) and stay responsive.

---

## 11. The Myth of "Parallel" Highlight/Complete

### 11.1 Where the highlighter is actually called

`engine.rs:1959` — inside `buffer_paint`, called from `repaint`, called from the event loop:

```rust
fn buffer_paint(&mut self, prompt: &dyn Prompt) -> Result<()> {
    let cursor_position_in_buffer = self.editor.insertion_point();
    let buffer_to_paint = self.editor.get_buffer();

    let mut styled_text = self
        .highlighter
        .highlight(buffer_to_paint, cursor_position_in_buffer);  // ← sync call, blocks loop
    // ...
}
```

The trait is:

```rust
// src/highlighter/mod.rs:10
pub trait Highlighter: Send {
    fn highlight(&self, line: &str, cursor: usize) -> StyledText;
}
```

`fn highlight` returns `StyledText` synchronously. There is **no thread, no rayon, no
`par_iter`, no `JoinHandle`**. A `grep -rn "thread\|spawn\|rayon\|par_iter" src/` confirms zero
worker-thread usage in highlighter, completer, hinter, validator, or menu code.

The "parallel feel" the user perceives is a side effect of the engine's poll-batching logic
(`engine.rs:889` — `event::poll(Duration::from_millis(0))`) which makes the editor *appear* to do
work between keystrokes. But every operation runs serially on the calling thread.

### 11.2 What this means for single-threaded async

Synchronous-on-the-event-loop is **strictly worse** for a single-threaded async target than a
multi-threaded design would be:

- A multi-threaded design could `spawn_blocking` the highlighter and let stdin keep flowing.
- A single-threaded async design **must** have the highlighter cooperatively yield, otherwise the
  whole event loop stalls until it returns.

So for nushell-class highlighters that currently take 10–50 ms, the migration is doing two things
simultaneously:

1. Removing the artificial blocking syscalls in `read_line_helper`.
2. Making the user-supplied callbacks async so they can be cancelled or paused mid-flight when a
   new keystroke arrives.

### 11.3 The "abort on new input" pattern

Modern fish/zsh-style shells run the highlighter **with cancellation**: as soon as the user types
another character, the in-flight highlight is dropped and a new one starts on the new buffer. In
sync Rust this requires threads + atomics. In async Rust it's a one-liner using `select`-style
combinators or just dropping the future:

```rust
// Pseudocode of the new event loop
loop {
    let next_event = event_source.next_event();           // Future<Output = Event>
    let highlight_task = highlighter.highlight(buffer);   // Future<Output = StyledText>

    futures_lite::future::race(
        async {
            let event = next_event.await;
            // dropping highlight_task here cancels it cooperatively
            handle_event(event);
        },
        async {
            let styled = highlight_task.await;
            painter.repaint(styled).await;
        },
    ).await;
}
```

This is **the central architectural improvement** of the async migration — not just non-blocking
stdin, but cancellable redrawing. It is impossible in the current sync design.

---

## 12. Tokio-Free Async: Concrete Choices

### 12.1 What Tokio gives us vs. what we actually need

Tokio bundles four things together:
1. A multi-threaded executor (`#[tokio::main]`).
2. A single-threaded executor (`current_thread::Runtime`).
3. A reactor (epoll/kqueue/IOCP wrapper via `mio`).
4. Async I/O primitives (`AsyncRead`/`AsyncWrite`, channels, timers).

For a single-threaded reedline that runs in WASM or alongside any caller-chosen runtime, we want
**none of these**. Specifically:

- The executor is the **caller's** problem. Reedline returns futures; the caller drives them.
- The reactor doesn't exist on WASM and is not needed for a callback-driven stdin.
- Channels and timers can use `futures-channel` and a caller-supplied timer (or none at all).

So the public API uses **only** `core::future::Future` and `core::pin::Pin`. The internal helpers
use `futures-core` (no executor, no reactor — just trait definitions for `Stream` and `Sink`).

### 12.2 Recommended dependencies (replacing `tokio`)

| Need | Tokio choice | Tokio-free choice | Notes |
|---|---|---|---|
| `Future`, `Pin`, `Waker` | (built-in) | (built-in) | `core::future` is in std since 1.36 |
| Stream trait | `tokio_stream::Stream` | `futures-core::Stream` | tiny, no executor |
| MPSC channel | `tokio::sync::mpsc` | `futures-channel::mpsc` | also no executor; works on WASM |
| Oneshot | `tokio::sync::oneshot` | `futures-channel::oneshot` | same |
| `select!` | `tokio::select!` | `futures::select!` or `futures-lite::race` | macro, no runtime |
| Timer / sleep | `tokio::time::sleep` | **caller-provided** via `Future` parameter | reedline never sleeps internally |
| spawn (background work) | `tokio::spawn` | **forbidden** | single-threaded; no background work |
| async traits | `async-trait` | RPITIT (Rust 1.75+) | bumps MSRV; see [§ 17](#17-msrv-considerations) |
| WASM glue | n/a | `wasm-bindgen-futures::spawn_local` | only inside `wasm` feature |

Total tokio-free runtime dependencies: `futures-core` (~5 kB), `futures-channel` (~10 kB). That's it.
For test-only `block_on` we use `pollster` (~1 kB, 100 lines of code) — never compiled into
the production library.

### 12.3 The `EventSource` trait — final form

```rust
// src/event_source.rs (new module, replacing the Phase 1/2 design)

use core::future::Future;
use core::pin::Pin;

/// Async-only source of terminal events. Drives the entire engine.
///
/// Implementors yield events one at a time. When no event is available, the
/// returned future is `Pending`; the caller's executor parks the task until
/// the underlying transport (epoll wakeup, JS callback, test driver) wakes it.
pub trait EventSource {
    /// Yield the next event. The future is dropped (and any registered waker
    /// detached) on cancellation, so the caller may race this against other
    /// futures freely.
    fn next_event(&mut self) -> Pin<Box<dyn Future<Output = std::io::Result<Event>> + '_>>;
}
```

Notes:

- **No `Send` bound.** The future is not `Send`, the trait object is not `Send`. This is
  deliberate: it permits `Rc<RefCell<T>>` inside implementations, which is mandatory for
  `wasm-bindgen` callbacks.
- **No associated type for the future.** Using a boxed future trades a tiny per-event allocation
  (~32 bytes) for object safety. With RPITIT (Rust 1.75) we could use `impl Future` and avoid the
  box, but every caller would lose the ability to store an `EventSource` as `Box<dyn EventSource>`.
  Given that the library has exactly one `EventSource` field per `Reedline` instance, the box
  cost is negligible.
- **No `poll_event` low-level method.** Implementations stay simple — no manual `Poll`.

### 12.4 Built-in back-ends (all optional, all single-threaded)

```
src/event_source/
├── mod.rs                  // trait + tiny re-exports
├── crossterm_native.rs     // feature = "crossterm-backend" (default for native)
├── wasm_dom.rs             // feature = "wasm-backend"     (browser)
└── memory.rs               // always-on; for tests + library users
```

The `memory.rs` back-end is just:

```rust
pub struct MemoryEventSource {
    queue: VecDeque<Event>,
    waker: Option<Waker>,
}

impl MemoryEventSource {
    pub fn push(&mut self, event: Event) {
        self.queue.push_back(event);
        if let Some(w) = self.waker.take() { w.wake(); }
    }
}
```

This **replaces** the test fixtures that currently use `event::read()` mocking — see § 16.

---

## 13. Complete Removal Inventory

The following items are **deleted outright** (no deprecation, no feature flag, no shim):

### 13.1 Public API removals

| Item | Location | Reason |
|---|---|---|
| `Reedline::read_line` | `engine.rs:751` | Replaced by `read_line` (now async) |
| `Reedline::with_break_signal` | `engine.rs:649` | Threading is gone; cancellation is async |
| `Reedline::with_idle_callback` | `engine.rs:2110` | Replaced by composing with caller's async work |
| `Reedline::with_external_printer` | `engine.rs:2057` | Replaced by an `async fn print(&mut self, line)` that the caller awaits |
| `Reedline::with_poll_interval` | `engine.rs:2081` | No more polling — events are awaited directly |
| `Signal::ExternalBreak` | `enums.rs` | No `break_signal` to produce it |
| `crate::ExternalPrinter` | `external_printer.rs` (whole file) | crossbeam-based; replaced |
| Re-exports of `crossterm::{KeyCode, KeyModifiers, Color}` | `lib.rs:317-320` | Detach public API from crossterm |

### 13.2 Internal removals

| Item | Location | Reason |
|---|---|---|
| `poll_interval: Duration` field | `engine.rs:~210` | No polling |
| `break_signal: Option<Arc<AtomicBool>>` field | `engine.rs:207` | No threads |
| `idle_callback: Option<Box<dyn FnMut()>>` field | `engine.rs:220` | Replaced by async composition |
| `external_printer: Option<ExternalPrinter<String>>` | `engine.rs` | Replaced |
| `EVENTS_THRESHOLD`, `POLL_WAIT` constants | `engine.rs` | No batched polling |
| Paste-batching while-loops in `read_line_helper` | `engine.rs:888-901` | Bracketed paste already produces `Event::Paste`; no batching needed |
| `crossbeam` dependency | `Cargo.toml:23` | Removed |
| `external_printer` feature | `Cargo.toml:48` | Removed |
| `idle_callback` feature | `Cargo.toml:49` | Removed |
| `Send` bound on `EditMode`, `Highlighter`, `Completer`, `Hinter`, `Validator`, `Prompt`, `History`, `Menu`, `Clipboard`, `SemanticPromptMarkers`, `HistoryItemExtraInfo` | various `mod.rs` | Single-threaded; pointless bound |
| `unsafe impl Send for HistoryCompleter<'_>` | `completion/history.rs:16` | Pointless |
| `concurrent_histories_are_threadsafe` test | `history/cursor.rs:554` | Test of threadedness; threads are gone |
| `thread_safe`, `thread_safe_with_idle_callback`, `break_signal_is_send` tests | `engine.rs:2229-2350` | Same |
| `Arc<Mutex<...>>` in painter unit tests | `painter.rs:1033, 1043, 1386` | Replaced by `Rc<RefCell<...>>` |

### 13.3 Tests removed (count)

A full sweep identifies **8 tests** to delete outright (all related to threading/Send), and
**~24 sync engine tests** that need to be rewritten as `#[pollster::test]` (or equivalent) async
tests. The remaining **~264 tests** test pure logic (`LineBuffer`, `EditStack`, `vi/parser`,
`menu_functions`, `keybindings`) and need no changes at all — they don't touch the event loop or
I/O. See [§ 16](#16-test-strategy) for the test plan.

---

## 14. The Complete New API (Concrete Form)

### 14.1 The `Event` type — owned, no crossterm in the public API

```rust
// src/event.rs (new)

/// A normalized terminal event. Owned by reedline; not a re-export of crossterm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { columns: u16, rows: u16 },
    FocusGained,
    FocusLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode { Char(char), Enter, Tab, Backspace, Left, Right, Up, Down, /* ... */ }

bitflags::bitflags! {
    pub struct KeyModifiers: u8 { const SHIFT = 1; const CONTROL = 2; const ALT = 4; }
}
```

The crossterm back-end converts `crossterm::event::Event` → `reedline::Event`. The WASM back-end
converts `web_sys::KeyboardEvent` → `reedline::Event`. The two back-ends never share types; both
target the same internal enum.

This **eliminates** the existing `ReedlineRawEvent` wrapper (`enums.rs:1055`), since the
filtering of `KeyEventKind::Release` and normalisation of `Repeat` → `Press` is done in the
crossterm back-end exclusively.

### 14.2 The `Reedline` async API — one method, one signal type

```rust
impl Reedline {
    pub fn create() -> Self { /* unchanged builder */ }

    pub fn with_event_source(mut self, src: Box<dyn EventSource>) -> Self { ... }
    pub fn with_painter(mut self, p: Box<dyn Painter>) -> Self { ... }
    pub fn with_history(mut self, h: Box<dyn History>) -> Self { ... }
    pub fn with_highlighter(mut self, h: Box<dyn Highlighter>) -> Self { ... }
    pub fn with_completer(mut self, c: Box<dyn Completer>) -> Self { ... }
    pub fn with_hinter(mut self, h: Box<dyn Hinter>) -> Self { ... }
    pub fn with_validator(mut self, v: Box<dyn Validator>) -> Self { ... }
    // ...

    /// THE single entry point. Returns a future the caller drives.
    pub async fn read_line(&mut self, prompt: &dyn Prompt) -> std::io::Result<Signal>;
}

pub enum Signal {
    Success(String),
    CtrlC,
    CtrlD,
    HostCommand(String),
}
```

No `ExternalBreak`. If the caller wants cancellation, they race `read_line` against their own
cancellation future:

```rust
// Caller-side cancellation — no library support needed
let result = futures_lite::future::or(
    line_editor.read_line(&prompt),
    async { my_shutdown_signal.await; Err(io::Error::from(io::ErrorKind::Interrupted)) },
).await;
```

### 14.3 The new async traits

```rust
// src/highlighter/mod.rs
pub trait Highlighter {
    fn highlight<'a>(
        &'a self,
        line: &'a str,
        cursor: usize,
    ) -> Pin<Box<dyn Future<Output = StyledText> + 'a>>;
}

// src/completion/base.rs
pub trait Completer {
    fn complete<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<Suggestion>> + 'a>>;

    fn partial_complete<'a>(/* same shape */);
    fn total_completions<'a>(/* same shape */);
}

// src/hinter/mod.rs
pub trait Hinter {
    fn handle<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
        history: &'a dyn History,
        use_ansi_coloring: bool,
        cwd: &'a str,
    ) -> Pin<Box<dyn Future<Output = String> + 'a>>;

    fn complete_hint(&self) -> String;       // sync — returns cached state
    fn next_hint_token(&self) -> String;     // sync — returns cached state
}

// src/validator/mod.rs
pub trait Validator {
    fn validate<'a>(
        &'a self,
        line: &'a str,
    ) -> Pin<Box<dyn Future<Output = ValidationResult> + 'a>>;
}

// src/history/base.rs
pub trait History {
    fn save<'a>(&'a mut self, h: HistoryItem) -> Pin<Box<dyn Future<Output = Result<HistoryItem>> + 'a>>;
    fn load<'a>(&'a self, id: HistoryItemId) -> Pin<Box<dyn Future<Output = Result<HistoryItem>> + 'a>>;
    fn search<'a>(&'a self, q: SearchQuery) -> Pin<Box<dyn Future<Output = Result<Vec<HistoryItem>>> + 'a>>;
    fn count<'a>(&'a self, q: SearchQuery) -> Pin<Box<dyn Future<Output = Result<i64>> + 'a>>;
    fn update<'a>(/* ... */);
    fn delete<'a>(/* ... */);
    fn sync<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + 'a>>;
    fn session(&self) -> Option<HistorySessionId>;  // sync — pure getter
}
```

**Trade-offs of `Pin<Box<dyn Future>>`:**

- ✅ Object-safe — works with `Box<dyn Highlighter>`.
- ✅ No `async-trait` macro dependency, no `?Send` workarounds.
- ✅ Compiles on stable Rust 1.63 (current MSRV).
- ❌ One small allocation per call. For a highlighter called once per keystroke, this is ~50 ns of
  overhead — negligible. If the MSRV is bumped to 1.75 we can switch the *non*-trait helper
  functions to `impl Future`, leaving only the trait methods boxed.

`async-trait` is **not needed** and not added. Hand-rolled `Pin<Box<dyn Future>>` keeps the
dependency footprint minimal.

### 14.4 The new event loop (≈ 60 lines, replaces the 200-line current `read_line_helper`)

```rust
async fn read_line_inner(&mut self, prompt: &dyn Prompt) -> io::Result<Signal> {
    self.painter.initialize_prompt_position(self.suspended_state.as_ref()).await?;
    self.suspended_state = None;
    self.hide_hints = false;
    self.repaint(prompt).await?;

    loop {
        // Single await point: the next event. No polling, no batching.
        // Cancellation simply drops this future.
        let raw_event = self.event_source.next_event().await?;

        // Pure synchronous translation — no allocation, no I/O.
        let Some(reedline_event) = self.edit_mode.parse_event(raw_event) else {
            continue;
        };

        // Dispatch — handle_event is async because it may call
        // History::save / Validator::validate / Completer::complete.
        match self.handle_event(prompt, reedline_event).await? {
            EventStatus::Exits(signal) => {
                if self.suspended_state.is_none() {
                    self.painter.move_cursor_to_end().await?;
                }
                return Ok(signal);
            }
            EventStatus::Handled => self.repaint(prompt).await?,
            EventStatus::Inapplicable => {}
        }
    }
}
```

**Notice what's gone:**

- The `needs_polling` branch (`engine.rs:864-885`). No more polling — `next_event().await` is the
  one yield point.
- The `EVENTS_THRESHOLD` paste-batching loop (`engine.rs:888-901`). Bracketed paste already
  produces a single `Event::Paste(String)` from crossterm; the manual batching is a workaround
  for terminals without bracketed paste, which we can drop or move into the back-end.
- The `break_signal.swap()` check (`engine.rs:815-823`). Caller races their own cancellation.
- The `external_printer` drain (`engine.rs:826-839`). Replaced by an explicit
  `async fn print(&mut self, line: String)` method on `Reedline` that callers `await` whenever
  they have a line to print.
- The `idle_callback` invocation (`engine.rs:811-813`). Caller composes their own work.

That single deletion shrinks `engine.rs` by ~150 lines.

---

## 15. Subsystem-by-Subsystem Migration

This section enumerates **every** subsystem of the codebase and what changes for the async-only,
single-threaded, Tokio-free target.

### 15.1 `core_editor/` (Editor, LineBuffer, EditStack, ClipBuffer)

**Status: zero changes.** Pure in-memory data structures. The `Send` bound on
`Clipboard: Send` and `EditStack<T> where T: Send` are removed.

### 15.2 `enums.rs` (ReedlineEvent, EditCommand, Signal)

**Changes:**

- Delete `Signal::ExternalBreak`.
- Delete `ReedlineRawEvent` wrapper struct (filtering moves into the crossterm back-end).
- Rename or replace `EditCommand` re-exports of `crossterm::event::KeyCode` etc. with the new
  reedline-owned types from `src/event.rs`.
- Drop `MouseButton::From<crossterm::event::MouseButton>` (the conversion lives in the back-end now).

### 15.3 `engine.rs`

**Major rewrite.** ~30% of the file (event loop, polling, threads, atomics) is deleted; the rest
(builder methods, dispatch logic, history navigation, menu interaction) becomes `async fn`.

Concrete checklist:

- [ ] Remove `break_signal`, `idle_callback`, `external_printer`, `poll_interval` fields.
- [ ] Remove all `Send`/`Arc`/`AtomicBool` references.
- [ ] Replace `terminal::enable_raw_mode()` calls with `self.event_source.enable_raw_mode().await?` (a new method on `EventSource`, no-op on memory/WASM back-ends).
- [ ] Convert `read_line` and `read_line_helper` to `async fn`.
- [ ] Convert `handle_event`, `handle_history_search_event`, `handle_editor_event`, `handle_mouse_click`, `submit_buffer`, `repaint`, `buffer_paint`, `history_search_paint`, `update_buffer_from_history`, `previous_history`, `next_history`, `enter_history_search`, `open_editor` to `async fn`.
- [ ] Replace `History::search` blocking calls with `.await`.
- [ ] Replace `child.wait()` in `open_editor` with a back-end-defined async process API (or remove the feature on WASM).

### 15.4 `painting/painter.rs`

**Changes:**

- Convert all `pub(crate) fn paint_line/repaint_buffer/clear_screen/clear_scrollback/move_cursor_to_end/print_external_message/print_crlf/initialize_prompt_position` to `async fn` returning `Pin<Box<dyn Future<Output=Result<()>>>>` or, internally, plain `async fn`.
- Generic over a `Write`-like async output trait. Define a minimal local trait:

```rust
pub trait AsyncWriter {
    fn write_all<'a>(
        &'a mut self,
        buf: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>>;
    fn flush<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>>;
}
```

This avoids both `tokio::io::AsyncWrite` and `futures::io::AsyncWrite`. Native back-end provides
a wrapper that buffers into memory and flushes synchronously to `Stderr` (since stderr writes
rarely block in raw-mode terminals); WASM back-end calls into a JS function.

- The `pub(crate) struct Painter` becomes `pub(crate) struct Painter<W: AsyncWriter>` to keep
  the writer pluggable.
- Test fixtures using `Arc<Mutex<Vec<MarkerCall>>>` (`painter.rs:1033-1062`) become
  `Rc<RefCell<Vec<MarkerCall>>>` — single-threaded, no lock poisoning, simpler.

### 15.5 `history/`

**Changes:**

- `History` trait becomes async (signatures in § 14.3).
- `FileBackedHistory` keeps its current synchronous file I/O implementation **but wraps it in
  `async fn` that calls the sync code directly**. On native single-threaded async, file I/O is
  acceptable to do synchronously inside an `async fn` — it's brief, and there is no other
  thread to starve. The async signature is for *interface compatibility* with the WASM
  back-end, which uses `IndexedDB` / `localStorage` and is genuinely async.
- `fd_lock` is kept for native (no thread requirement; just OS file locking).
- `SqliteBackedHistory` is gated behind `cfg(not(target_arch = "wasm32"))` because `rusqlite` does
  not compile to WASM.
- A new `MemoryHistory` is added (always available, including WASM) — a simple `Vec<HistoryItem>`
  in RAM, useful for tests and for WASM environments without persistent storage.

### 15.6 `completion/`, `hinter/`, `highlighter/`, `validator/`, `prompt/`

All trait methods become async (`Pin<Box<dyn Future>>`). `DefaultCompleter`, `ExampleHighlighter`,
`DefaultHinter`, `CwdAwareHinter`, `DefaultValidator`, `DefaultPrompt` keep their pure-Rust
implementations and just box-wrap synchronous bodies:

```rust
impl Highlighter for ExampleHighlighter {
    fn highlight<'a>(&'a self, line: &'a str, _cursor: usize)
        -> Pin<Box<dyn Future<Output = StyledText> + 'a>>
    {
        let result = /* existing synchronous body */;
        Box::pin(async move { result })
    }
}
```

Cost per call: one `Box` allocation (~50 ns). On the keystroke hot path that is one allocation
per ~50 ms keypress — invisible.

### 15.7 `edit_mode/` (Emacs, Vi, Helix, keybindings)

**Status: zero logic changes.** `EditMode::parse_event` stays synchronous; it does no I/O.
Remove the `Send` bound. Replace `crossterm::event::Event` parameter type with the new
reedline-owned `Event` type (or, if we keep crossterm types internally for now, leave it and
only change the public boundary).

The `Helix` mode's `TryFrom<ReedlineRawEvent> for KeyEvent` impl gets simpler since
`ReedlineRawEvent` is gone.

### 15.8 `menu/`

**Status: zero changes.** Menus operate on already-fetched suggestions; they don't await. The
`Send` bound on `Menu` is removed.

`update_values(editor, completer, history)` — currently it takes `&mut dyn Completer` and calls
`completer.complete()` synchronously. Now `complete` is async, so this call site must move into
an `async fn` and await. That's a localised change in `engine.rs` where menu updates are
triggered (~6 call sites in `handle_editor_event`).

### 15.9 `terminal_extensions/`

**Changes:**

- `BracketedPasteGuard`, `KittyProtocolGuard`: their `enter`/`exit` methods write escape
  sequences via `execute!(stdout(), ...)`. Move these into the crossterm back-end (since they
  are only meaningful when crossterm is the back-end). On WASM and memory back-ends they are
  no-ops.
- `kitty_protocol_available()` stays as a native-only utility.

### 15.10 `external_printer.rs`

**Deleted entirely.** Replacement: `Reedline::print(&mut self, line: String) -> impl Future`
which takes the line, queues a repaint that includes the line above the prompt, and returns when
the redraw is complete. Since reedline is async-only and single-threaded, the caller awaits this
inline rather than sending across a channel.

For the use case of "another `async fn` produces output while `read_line` is awaited", the caller
uses `select`-style composition:

```rust
// Caller's pattern (no library threading needed)
loop {
    futures_lite::future::or(
        async { line = line_editor.read_line(&prompt).await?; Action::Input(line) },
        async { msg  = my_log_stream.next().await;          Action::Log(msg) },
    ).await;
}
```

If the user wants log output to interleave *during* a `read_line` call, they don't use
`read_line` — they use the lower-level `step()` API:

```rust
impl Reedline {
    /// Process exactly one event from the event source. Returns `Some(Signal)`
    /// if the line is complete, else `None`.
    pub async fn step(&mut self, prompt: &dyn Prompt) -> io::Result<Option<Signal>>;
}
```

This makes "external printing" trivial: between `step()` calls the caller can do whatever they
want, including printing log lines via `Reedline::print_above_prompt(&mut self, ...)`.

### 15.11 Build/feature changes

```toml
# Cargo.toml — proposed final state
[package]
name = "reedline"
edition = "2021"
rust-version = "1.75.0"  # bumped from 1.63 to allow `let ... else` and to drop async-trait

[dependencies]
# Always-on, all WASM-compatible:
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }
futures-core   = { version = "0.3", default-features = false }
futures-channel = { version = "0.3", default-features = false, features = ["alloc"] }
itertools = "0.13"
nu-ansi-term = "0.50"
serde = { version = "1", features = ["derive"] }
strip-ansi-escapes = "0.2"
strum = { version = "0.27", features = ["derive"] }
thiserror = "2"
unicase = "2"
unicode-segmentation = "1"
unicode-width = "0.2"
bitflags = "2"  # for the new KeyModifiers type

# Native-only, optional:
crossterm = { version = "0.29", optional = true, features = ["serde", "event-stream"] }
fd-lock   = { version = "4",  optional = true }
arboard   = { version = "3",  optional = true, default-features = false, features = ["wayland-data-control"] }
rusqlite  = { version = "0.37", optional = true }
serde_json = { version = "1", optional = true }

# WASM-only, optional:
wasm-bindgen          = { version = "0.2",  optional = true }
wasm-bindgen-futures  = { version = "0.4",  optional = true }
web-sys               = { version = "0.3",  optional = true, features = ["KeyboardEvent", "Element"] }
js-sys                = { version = "0.3",  optional = true }

# Helix mode, unrelated to async:
keybindings = { version = "0.0.2", optional = true }

[dev-dependencies]
pollster = "0.3"  # 100-line block_on, no executor; for tests only
rstest   = { version = "0.23", default-features = false }
tempfile = "3"
pretty_assertions = "1.4"

[features]
default        = ["crossterm-backend"]
crossterm-backend = ["crossterm", "fd-lock"]
wasm-backend   = ["wasm-bindgen", "wasm-bindgen-futures", "web-sys", "js-sys"]
sqlite         = ["rusqlite/bundled", "serde_json"]
sqlite-dynlib  = ["rusqlite", "serde_json"]
system_clipboard = ["arboard"]
helix          = ["keybindings"]
bashisms       = []
# REMOVED: external_printer, idle_callback, libc
```

Removed dependencies:

- ❌ `crossbeam` — gone (deleted with `external_printer`).
- ❌ `tokio` — never present, never added.
- ❌ `async-trait` — avoided via hand-rolled `Pin<Box<dyn Future>>`.

---

## 16. Test Strategy

### 16.1 Test classification

Of the **296** existing `#[test]` functions across the codebase, classification by impact is:

| Category | Count | Action |
|---|---|---|
| Pure-logic tests (no event loop, no I/O) | ~256 | **No changes.** `LineBuffer`, `EditStack`, `vi/parser`, `keybindings`, `menu_functions`, `text_manipulation`, `prompt_lines`, `utils`, `styled_text`, etc. |
| Tests that use the engine's high-level API but no real terminal | ~24 | **Convert to `async`.** Rewrite as `#[pollster::test]` or wrap with `pollster::block_on(async { ... })`. Call sites change from `reedline.read_line(...)` to `reedline.read_line(...).await`. |
| Tests of threadedness (`thread_safe`, `thread_safe_with_idle_callback`, `break_signal_is_send`, `concurrent_histories_are_threadsafe`) | 4 | **Delete outright.** They test removed behaviour. |
| Tests of `idle_callback`, `external_printer`, `break_signal` builders | ~4 | **Delete outright.** Features are removed. |
| Painter tests using `Arc<Mutex<...>>` | 8 | **Convert** to `Rc<RefCell<...>>` (and become `async`). |

Net delta: **~8 tests deleted, ~32 tests rewritten as async, ~256 tests untouched**.

### 16.2 The async test pattern

```rust
// Before
#[test]
fn ctrl_d_on_empty_buffer_returns_ctrl_d_signal() {
    let mut reedline = Reedline::create();
    /* ... feed events synchronously via test fixture ... */
    let signal = reedline.read_line(&prompt).unwrap();
    assert!(matches!(signal, Signal::CtrlD));
}

// After
#[test]
fn ctrl_d_on_empty_buffer_returns_ctrl_d_signal() {
    pollster::block_on(async {
        let memory_source = MemoryEventSource::with_events(vec![
            Event::Key(KeyEvent::ctrl('d')),
        ]);
        let mut reedline = Reedline::create().with_event_source(Box::new(memory_source));
        let signal = reedline.read_line(&prompt).await.unwrap();
        assert!(matches!(signal, Signal::CtrlD));
    });
}
```

Why `pollster` over `tokio::test` or `async-std::test`:

- ~100 lines of code, zero dependencies of its own.
- Runs **single-threaded** — matches the production target.
- No reactor — so a test that *forgets* to drain the event source deadlocks immediately,
  surfacing bugs that a Tokio reactor would hide.
- Dev-dep only — never compiled into the published library.

### 16.3 New tests required

The migration adds **about 40 new tests** covering the async-only behaviours that did not exist before:

1. **Cancellation tests** (~10): drop `read_line` future at various points; verify state is consistent.
2. **`MemoryEventSource` tests** (~6): waker registration, drain semantics, capacity behaviour.
3. **Async-trait round-trip tests** (~6): a custom `Highlighter` that yields once via `pending().poll()` and verifies the engine cooperatively continues.
4. **`step()`-API tests** (~6): low-level single-event processing.
5. **History async-trait tests** (~4): re-using the existing logic of `History` tests, just with `.await`.
6. **WASM smoke tests** (~8): under `cfg(target_arch = "wasm32")`, run with `wasm-bindgen-test`.

### 16.4 CI matrix

```
matrix:
  os:    [ubuntu-latest, macos-latest, windows-latest]
  rust:  [1.75.0, stable]
  features:
    - "default"                          # crossterm-backend
    - "default,bashisms,system_clipboard,sqlite"
    - "wasm-backend"                     # wasm32-unknown-unknown only
  exclude:
    - features: "wasm-backend"
      os: windows-latest                 # build only on Linux for WASM
```

`wasm-bindgen-test` is run via `wasm-pack test --node` for the WASM target — no browser needed.

---

## 17. MSRV Considerations

| Feature used | Minimum Rust | Notes |
|---|---|---|
| `core::future::Future` | 1.36 | OK on current MSRV 1.63 |
| `Pin<Box<dyn Future>>` | 1.36 | OK |
| `async fn` in inherent impls | 1.39 | OK |
| `let ... else` (used in new event loop) | 1.65 | needs MSRV bump |
| RPITIT (`fn foo() -> impl Future`) in **traits** | 1.75 | only needed if we want `impl Future` in trait methods; we don't — we use `Pin<Box<dyn Future>>` |
| `async fn` in **traits** without `async-trait` | 1.75 | not used |

**Recommendation:** bump MSRV to **1.75.0**, which is broadly acceptable in late-2025 ecosystems
and unlocks RPITIT for *internal* helper functions (inside `engine.rs` and back-ends), keeping
the trait-method box overhead but removing it from internal hot paths.

If the MSRV must stay at 1.63, everything still works — `Pin<Box<dyn Future>>` is sufficient
everywhere. The cost is a few extra allocations per keystroke (still negligible).

---

## 18. Migration Effort (Revised)

| Work item | Effort | Notes |
|---|---|---|
| Define new `Event`, `KeyCode`, `KeyModifiers`, `EventSource` types | 1–2 days | Mechanical |
| Convert all 6 plug-in traits to async | 1 day | Bodies are unchanged; just wrap in `Box::pin(async move { ... })` |
| Rewrite `engine.rs` event loop + delete polling/threads/atomics | 2–3 days | The single biggest change |
| Convert `Painter` to async writer | 1–2 days | Mechanical, lots of `.await`s |
| Convert `History` to async + native impls + add `MemoryHistory` | 2 days | |
| Crossterm back-end (`event_source/crossterm_native.rs`) | 1 day | Wraps `crossterm::event::EventStream` |
| WASM back-end (`event_source/wasm_dom.rs` + `WasmPainter`) | 4–6 days | New code, requires JS interop tests |
| Delete dead code (`external_printer.rs`, threading tests, Send bounds) | 0.5 day | |
| Convert ~32 affected tests to async (`pollster::block_on`) | 2 days | |
| Add ~40 new tests (cancellation, MemoryEventSource, async-trait composition) | 3 days | |
| Update README, lib.rs doctests, examples (all 19 examples) | 2 days | Each example becomes `pollster::block_on(async { ... })` |
| **Total** | **~20–25 days** | One person, including review and CI |

This is **less work than the Part I plan** because we are not maintaining two parallel APIs.

---

## 19. Risks and Mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Highlighter authors find `Pin<Box<dyn Future>>` ergonomically awkward | High | Low | Provide a `BoxedHighlighter` type alias and an `async-fn-style` macro `impl_highlighter!` |
| Removing the sync API breaks downstream nushell | Certain | High | nushell is the primary user; coordinate the breaking version bump (0.50? 1.0?) and provide a migration guide. The task statement explicitly accepts breakage. |
| `crossterm::event::EventStream` requires *some* runtime even on native (it spawns a futures-internal task) | Medium | Medium | The `event-stream` feature uses `futures::stream::Stream`; it doesn't require Tokio. Verified: it uses `mio` on a thread but only for the read side; the public surface is just a `Stream`. We accept a *single* internal mio thread on native (it is part of crossterm, not reedline). On WASM crossterm is not used at all. |
| `wasm32-unknown-unknown` builds break because of transitively-required system crates | High | Medium | Make all native-only crates (`fd_lock`, `crossterm`, `arboard`, `rusqlite`) optional and gated behind features that are off by default in WASM builds. The default feature `crossterm-backend` is replaced by `wasm-backend` in the user's `Cargo.toml`. |
| Cancellation semantics surprise users (dropped futures may leave history half-saved) | Medium | Medium | Document that `History::save` futures must be drop-safe. The `FileBackedHistory` impl already is (it appends to a buffered writer that flushes on `sync()`); `SqliteBackedHistory` already wraps in transactions that auto-rollback. |
| Performance regression from `Box::pin` allocations | Low | Low | Microbenchmark a hot path (1000 keystrokes). Expected overhead: ~50 µs per keystroke total, well under any human-perceptible threshold. |

---

## 20. Summary of Part II

The revised constraints simplify the design:

1. **No parallelism today, none added.** Highlighters and completers are sync today, will be
   async tomorrow. The async-ness is for *cooperative yielding and cancellation*, not for
   concurrent execution. This is exactly right for a single-threaded WASM target.

2. **Tokio is unnecessary.** The library returns `Future`s; the caller drives them. Internal
   plumbing uses only `core::future`, `futures-core`, and `futures-channel`. Total async-runtime
   cost: ~15 kB of trait-and-channel code, no executor.

3. **No backward compatibility shrinks the codebase.** ~150 lines deleted from `engine.rs`, plus
   `external_printer.rs` (72 lines), plus 8 tests, plus the `idle_callback`/`external_printer`/
   `libc` features and the `crossbeam` dependency. The API gets *smaller* and easier to learn.

4. **The `Send` bound is removed everywhere.** This is the single most important enabler of WASM
   support — `wasm-bindgen` callbacks produce `Rc<RefCell<T>>` capture closures that are not
   `Send`, and forcing `Send` on traits would make WASM back-ends unwriteable.

5. **Effort estimate: ~20–25 person-days.** Lower than the layered Part I plan because we are
   building one API, not two.

The single largest design decision is **`Pin<Box<dyn Future>>` for trait methods** rather than
`async-trait` or RPITIT. This keeps MSRV flexibility, avoids macro overhead in user code, and
costs one allocation per call — invisible in practice.