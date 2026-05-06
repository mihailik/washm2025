//! Vanilla-FFI WASM bridge for reedline.
//!
//! The highlighter and completer are not implemented in Rust here. Instead we
//! implement reedline's `Highlighter` and `Completer` traits as thin shims that
//! call out to the JavaScript host through `extern "C"` imports, which lets the
//! host plug in any language service it wants (the demo wires up the
//! TypeScript Language Service for both Node and the browser).
use futures_executor::block_on;
use reedline::{Completer, EditCommand, Editor, Highlighter, Span, StyledText, Suggestion};
use std::cell::RefCell;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;

// ---------------------------------------------------------------------------
// Shared I/O buffer used to ferry UTF-8 bytes between Rust and the JS host.
// ---------------------------------------------------------------------------
const IO_CAP: usize = 65536;
static mut IO_BUF: [u8; IO_CAP] = [0; IO_CAP];

#[no_mangle]
pub extern "C" fn reedline_io_buf_ptr() -> *mut u8 {
    unsafe { IO_BUF.as_mut_ptr() }
}

#[no_mangle]
pub extern "C" fn reedline_io_buf_cap() -> usize {
    IO_CAP
}

// ---------------------------------------------------------------------------
// Host imports. The JS side fills `IO_BUF` with its response and returns the
// number of bytes written.
// ---------------------------------------------------------------------------
extern "C" {
    /// Highlight `IO_BUF[..in_len]`. The host writes ANSI-decorated UTF-8 back
    /// into `IO_BUF` starting at offset 0 and returns the byte length.
    fn host_highlight(in_len: usize, cursor: usize) -> usize;

    /// Complete `IO_BUF[..in_len]` at byte offset `cursor`. The host writes
    /// suggestions back as `\n`-separated UTF-8 strings; the first line of the
    /// payload is the replacement-span as `start,end` followed by a single
    /// `\x1f` separator, then the suggestions themselves. If there are no
    /// suggestions the host returns 0.
    fn host_complete(in_len: usize, cursor: usize) -> usize;
}

fn copy_into_io(buf: &str) -> usize {
    let bytes = buf.as_bytes();
    let n = bytes.len().min(IO_CAP);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), IO_BUF.as_mut_ptr(), n);
    }
    n
}

fn read_io_string(len: usize) -> String {
    let len = len.min(IO_CAP);
    let slice = unsafe { &IO_BUF[..len] };
    String::from_utf8_lossy(slice).into_owned()
}

// ---------------------------------------------------------------------------
// Reedline trait implementations that delegate to the JS host.
// ---------------------------------------------------------------------------
struct JsHighlighter;

impl Highlighter for JsHighlighter {
    fn highlight<'a>(
        &'a self,
        line: &'a str,
        cursor: usize,
    ) -> Pin<Box<dyn Future<Output = StyledText> + 'a>> {
        Box::pin(async move {
            let in_len = copy_into_io(line);
            let out_len = unsafe { host_highlight(in_len, cursor) };
            let rendered = read_io_string(out_len);
            // The host already produced ANSI-styled UTF-8, so we return it as a
            // single span with the default style. `render_simple` will emit the
            // bytes verbatim because `Style::default()` adds no SGR codes.
            let mut st = StyledText::new();
            st.push((nu_ansi_term::Style::default(), rendered));
            st
        })
    }
}

struct JsCompleter;

impl Completer for JsCompleter {
    fn complete<'a>(
        &'a mut self,
        line: &'a str,
        pos: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<Suggestion>> + 'a>> {
        Box::pin(async move {
            let in_len = copy_into_io(line);
            let out_len = unsafe { host_complete(in_len, pos) };
            if out_len == 0 {
                return Vec::new();
            }
            let payload = read_io_string(out_len);
            // Header: `start,end\x1f` followed by `\n`-separated suggestion
            // values. If the header is absent we fall back to span(pos, pos).
            let (header, body) = match payload.split_once('\x1f') {
                Some((h, b)) => (Some(h), b),
                None => (None, payload.as_str()),
            };
            let span = header
                .and_then(|h| h.split_once(','))
                .and_then(|(a, b)| Some(Span::new(a.parse().ok()?, b.parse().ok()?)))
                .unwrap_or(Span::new(pos, pos));
            body.split('\n')
                .filter(|s| !s.is_empty())
                .map(|s| Suggestion {
                    value: s.to_string(),
                    span,
                    ..Default::default()
                })
                .collect()
        })
    }
}

// ---------------------------------------------------------------------------
// Engine state.
// ---------------------------------------------------------------------------
struct State {
    editor: Editor,
    highlighter: JsHighlighter,
    completer: JsCompleter,
    history: Vec<String>,
    history_cursor: i32,
    last_submission: String,
    scratch: String,
    scratch_kind: ScratchKind,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ScratchKind {
    Buffer,
    Highlighted,
    Completions,
}

impl State {
    fn new() -> Self {
        Self {
            editor: Editor::default(),
            highlighter: JsHighlighter,
            completer: JsCompleter,
            history: Vec::new(),
            history_cursor: -1,
            last_submission: String::new(),
            scratch: String::new(),
            scratch_kind: ScratchKind::Buffer,
        }
    }
    fn refresh_buffer(&mut self) {
        self.scratch.clear();
        self.scratch.push_str(self.editor.line_buffer().get_buffer());
        self.scratch_kind = ScratchKind::Buffer;
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

const KEY_BASE: u32 = 0x20_0000;
const KEY_ENTER: u32 = KEY_BASE + 1;
const KEY_BACKSPACE: u32 = KEY_BASE + 2;
const KEY_TAB: u32 = KEY_BASE + 3;
const KEY_LEFT: u32 = KEY_BASE + 4;
const KEY_RIGHT: u32 = KEY_BASE + 5;
const KEY_UP: u32 = KEY_BASE + 6;
const KEY_DOWN: u32 = KEY_BASE + 7;
const KEY_HOME: u32 = KEY_BASE + 8;
const KEY_END: u32 = KEY_BASE + 9;
const KEY_DELETE: u32 = KEY_BASE + 10;
const KEY_ESC: u32 = KEY_BASE + 11;
const MOD_CTRL: u32 = 1;
const _MOD_ALT: u32 = 2;
const _MOD_SHIFT: u32 = 4;
const RES_OK: u32 = 0;
const RES_SUBMIT: u32 = 1;
const RES_CANCEL: u32 = 2;

#[no_mangle]
pub extern "C" fn reedline_init() {
    STATE.with(|s| {
        *s.borrow_mut() = State::new();
    });
}

#[no_mangle]
pub extern "C" fn reedline_alloc(len: usize) -> *mut u8 {
    let mut buf = ManuallyDrop::new(Vec::<u8>::with_capacity(len));
    buf.as_mut_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn reedline_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let _ = Vec::from_raw_parts(ptr, len, len);
}

#[no_mangle]
pub extern "C" fn reedline_process_key(key: u32, modifiers: u32) -> u32 {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let res = handle_key(&mut state, key, modifiers);
        state.refresh_buffer();
        res
    })
}

fn handle_key(state: &mut State, key: u32, modifiers: u32) -> u32 {
    let ctrl = modifiers & MOD_CTRL != 0;
    if key < KEY_BASE {
        if let Some(c) = std::char::from_u32(key) {
            if ctrl {
                match c.to_ascii_lowercase() {
                    'a' => state
                        .editor
                        .run_edit_command_pub(&EditCommand::MoveToLineStart { select: false }),
                    'e' => state
                        .editor
                        .run_edit_command_pub(&EditCommand::MoveToLineEnd { select: false }),
                    'h' => state.editor.run_edit_command_pub(&EditCommand::Backspace),
                    'c' => return RES_CANCEL,
                    _ => {}
                }
                return RES_OK;
            }
            if !c.is_control() {
                state
                    .editor
                    .run_edit_command_pub(&EditCommand::InsertChar(c));
            }
            return RES_OK;
        }
    }

    match key {
        KEY_ENTER => {
            let text = state.editor.line_buffer().get_buffer().to_string();
            state.last_submission = text.clone();
            if !text.trim().is_empty() {
                state.history.push(text);
            }
            state.history_cursor = -1;
            state.editor.run_edit_command_pub(&EditCommand::Clear);
            return RES_SUBMIT;
        }
        KEY_BACKSPACE => state.editor.run_edit_command_pub(&EditCommand::Backspace),
        KEY_DELETE => state.editor.run_edit_command_pub(&EditCommand::Delete),
        // Tab is a no-op at the editor level. The JS host layer is in charge
        // of interpreting it (typically as "apply the first completion").
        KEY_TAB => {}
        KEY_LEFT => state
            .editor
            .run_edit_command_pub(&EditCommand::MoveLeft { select: false }),
        KEY_RIGHT => state
            .editor
            .run_edit_command_pub(&EditCommand::MoveRight { select: false }),
        KEY_HOME => state
            .editor
            .run_edit_command_pub(&EditCommand::MoveToLineStart { select: false }),
        KEY_END => state
            .editor
            .run_edit_command_pub(&EditCommand::MoveToLineEnd { select: false }),
        KEY_UP => history_prev(state),
        KEY_DOWN => history_next(state),
        KEY_ESC => state.editor.run_edit_command_pub(&EditCommand::Clear),
        _ => {}
    }
    RES_OK
}

fn history_prev(state: &mut State) {
    if state.history.is_empty() {
        return;
    }
    let new = if state.history_cursor < 0 {
        state.history.len() as i32 - 1
    } else if state.history_cursor > 0 {
        state.history_cursor - 1
    } else {
        0
    };
    state.history_cursor = new;
    let line = state.history[new as usize].clone();
    state.editor.run_edit_command_pub(&EditCommand::Clear);
    for c in line.chars() {
        state
            .editor
            .run_edit_command_pub(&EditCommand::InsertChar(c));
    }
}

fn history_next(state: &mut State) {
    if state.history_cursor < 0 {
        return;
    }
    let len = state.history.len() as i32;
    state.history_cursor += 1;
    if state.history_cursor >= len {
        state.history_cursor = -1;
        state.editor.run_edit_command_pub(&EditCommand::Clear);
        return;
    }
    let line = state.history[state.history_cursor as usize].clone();
    state.editor.run_edit_command_pub(&EditCommand::Clear);
    for c in line.chars() {
        state
            .editor
            .run_edit_command_pub(&EditCommand::InsertChar(c));
    }
}

#[no_mangle]
pub extern "C" fn reedline_buffer_ptr() -> *const u8 {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.refresh_buffer();
        state.scratch.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn reedline_buffer_len() -> usize {
    STATE.with(|s| {
        let st = s.borrow();
        if st.scratch_kind == ScratchKind::Buffer {
            st.scratch.len()
        } else {
            st.editor.line_buffer().get_buffer().len()
        }
    })
}

#[no_mangle]
pub extern "C" fn reedline_highlighted_ptr() -> *const u8 {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let buffer = state.editor.get_buffer().to_string();
        let cursor = state.editor.line_buffer().insertion_point();
        let highlighted = block_on(state.highlighter.highlight(&buffer, cursor));
        state.scratch = highlighted.render_simple();
        state.scratch_kind = ScratchKind::Highlighted;
        state.scratch.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn reedline_highlighted_len() -> usize {
    STATE.with(|s| s.borrow().scratch.len())
}

#[no_mangle]
pub extern "C" fn reedline_completions_ptr() -> *const u8 {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let buffer = state.editor.get_buffer().to_string();
        let cursor = state.editor.line_buffer().insertion_point();
        let suggestions = block_on(state.completer.complete(&buffer, cursor));
        state.scratch = suggestions
            .into_iter()
            .map(|s| s.value)
            .collect::<Vec<_>>()
            .join("\n");
        state.scratch_kind = ScratchKind::Completions;
        state.scratch.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn reedline_completions_len() -> usize {
    STATE.with(|s| s.borrow().scratch.len())
}

#[no_mangle]
pub extern "C" fn reedline_cursor() -> usize {
    STATE.with(|s| s.borrow().editor.line_buffer().insertion_point())
}

#[no_mangle]
pub extern "C" fn reedline_last_submission_ptr() -> *const u8 {
    STATE.with(|s| s.borrow().last_submission.as_ptr())
}

#[no_mangle]
pub extern "C" fn reedline_last_submission_len() -> usize {
    STATE.with(|s| s.borrow().last_submission.len())
}
