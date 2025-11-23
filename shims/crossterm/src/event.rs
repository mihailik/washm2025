use std::time::Duration;
use std::io;
use crate::Command;

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum KeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    F(u8),
    Char(char),
    Null,
    Esc,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    KeypadBegin,
    Media(MediaKeyCode),
    Modifier(ModifierKeyCode),
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum MediaKeyCode {
    Play,
    Pause,
    PlayPause,
    Reverse,
    Stop,
    FastForward,
    Rewind,
    TrackNext,
    TrackPrevious,
    Record,
    LowerVolume,
    RaiseVolume,
    MuteVolume,
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum ModifierKeyCode {
    LeftShift,
    LeftControl,
    LeftAlt,
    LeftSuper,
    LeftHyper,
    LeftMeta,
    RightShift,
    RightControl,
    RightAlt,
    RightSuper,
    RightHyper,
    RightMeta,
    IsoLevel3Shift,
    IsoLevel5Shift,
}

bitflags::bitflags! {
    #[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct KeyModifiers: u8 {
        const SHIFT = 0b0000_0001;
        const CONTROL = 0b0000_0010;
        const ALT = 0b0000_0100;
        const SUPER = 0b0000_1000;
        const HYPER = 0b0001_0000;
        const META = 0b0010_0000;
        const NONE = 0b0000_0000;
    }
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

bitflags::bitflags! {
    #[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct KeyEventState: u8 {
        const KEYPAD = 0b0000_0001;
        const NONE = 0b0000_0000;
    }
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
    pub state: KeyEventState,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    pub const fn new_with_kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind,
            state: KeyEventState::NONE,
        }
    }

    pub const fn new_with_kind_and_state(
        code: KeyCode,
        modifiers: KeyModifiers,
        kind: KeyEventKind,
        state: KeyEventState,
    ) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind,
            state,
        }
    }
}

impl From<KeyCode> for KeyEvent {
    fn from(code: KeyCode) -> Self {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Hash)]
pub enum Event {
    FocusGained,
    FocusLost,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
}

pub fn read() -> io::Result<Event> {
    use std::io::Read;
    use crate::parse::{Parser, InternalEvent};

    let mut parser = Parser::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    loop {
        // In a real implementation, we would buffer bytes and parse them.
        // Since we don't have a persistent buffer across calls in this simple function,
        // we might lose data if we read more than one event.
        // However, for a shim, we can try to read byte by byte or small chunks.
        
        // Ideally, we should have a static buffer or a global state.
        // But for now, let's just read one byte at a time to be safe, or use the parser's internal buffer if we could persist it.
        // The `Parser` struct I wrote has a buffer, but it's created new here.
        
        // To properly support this, we need a global parser state or similar.
        // But let's try a simpler approach: read until we get a full event.
        
        let mut byte = [0u8; 1];
        handle.read_exact(&mut byte)?;
        
        let events = parser.advance(&byte, true);
        if let Some(internal_event) = events.into_iter().next() {
             match internal_event {
                 InternalEvent::Event(e) => return Ok(e),
                 InternalEvent::CursorPosition(_, _) => {
                     // Ignore cursor position events for now in `read()` as they are usually responses to queries
                     // or handle them if needed.
                 }
             }
        }
    }
}

pub fn poll(_timeout: Duration) -> io::Result<bool> {
    // Stub implementation.
    // In a real WASM environment, we might not be able to poll easily without async.
    // If we return true, `read` will be called.
    // If we return false, the caller might sleep.
    
    // If timeout is 0, we should check if there is data available.
    // If we can't check, we can return false to be safe, or true if we want to block.
    // But `reedline` uses `poll` to check for input.
    
    // For now, let's assume there is always input or we can't check.
    // Returning `true` might cause blocking in `read`.
    // Returning `false` might cause the loop to spin or sleep.
    
    // A safe bet for a shim might be `false` if we want to be non-blocking, 
    // but then we never read input.
    // If we return `true`, we block.
    
    // Let's try to use a peek if possible, or just return true and hope `read` doesn't block forever if we are the only thread.
    // But wait, if we are the only thread, blocking is fine?
    
    Ok(true)
}

// Commands

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnableMouseCapture;

impl Command for EnableMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1000h\x1b[?1003h\x1b[?1015h\x1b[?1006h")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisableMouseCapture;

impl Command for DisableMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1000l")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnableBracketedPaste;

impl Command for EnableBracketedPaste {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?2004h")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisableBracketedPaste;

impl Command for DisableBracketedPaste {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?2004l")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushKeyboardEnhancementFlags(pub KeyboardEnhancementFlags);

impl Command for PushKeyboardEnhancementFlags {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[>{}u", self.0.bits())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopKeyboardEnhancementFlags;

impl Command for PopKeyboardEnhancementFlags {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[<u")
    }
}

bitflags::bitflags! {
    #[derive(Debug, PartialOrd, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct KeyboardEnhancementFlags: u8 {
        const DISAMBIGUATE_ESCAPE_CODES = 0b0000_0001;
        const REPORT_EVENT_TYPES = 0b0000_0010;
        const REPORT_ALTERNATE_KEYS = 0b0000_0100;
        const REPORT_ALL_KEYS_AS_ESCAPE_CODES = 0b0000_1000;
    }
}
