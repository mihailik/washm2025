use crate::bridges::eval::{eval_blocking, WasmHostFuture};
use std::io::{self, Write};
use serde::Deserialize;
use crate::brush_interactive::{InputBackend, ReadResult, InteractivePrompt, ShellError, ShellRef};

/// Event types parsed from raw input
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Char(char),
    Arrow(Direction),
    Backspace,
    Delete,
    Enter,
    Ctrl(char),
    Tab,
    Esc,
}

/// Arrow key directions
#[derive(Debug, Clone, PartialEq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Line editor state with cursor tracking
struct LineBuffer {
    chars: Vec<char>,
    cursor: usize,
}

impl LineBuffer {
    fn new() -> Self {
        Self {
            chars: Vec::new(),
            cursor: 0,
        }
    }

    fn from_string(s: String) -> Self {
        let chars: Vec<char> = s.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
            true
        } else {
            false
        }
    }

    fn delete(&mut self) -> bool {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
            true
        } else {
            false
        }
    }

    fn move_cursor(&mut self, n: isize) {
        self.cursor = self.cursor.saturating_add_signed(n).min(self.chars.len());
    }

    fn to_string(&self) -> String {
        self.chars.iter().collect()
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }
}

/// Input event parser state machine
#[derive(Clone)]
struct EventParser {
    state: ParserState,
}

#[derive(Debug, Clone, PartialEq)]
enum ParserState {
    Normal,
    EscapeStart,
    CsiStart,
    CsiDelete, // Intermediate state for ESC[3~
}

impl EventParser {
    fn new() -> Self {
        Self {
            state: ParserState::Normal,
        }
    }

    fn parse(&mut self, c: char) -> Option<Event> {
        match self.state {
            ParserState::Normal => {
                match c {
                    '\x1b' => {
                        self.state = ParserState::EscapeStart;
                        None
                    }
                    '\r' | '\n' => Some(Event::Enter),
                    '\x08' | '\x7f' => Some(Event::Backspace),
                    '\t' => Some(Event::Tab),
                    '\x03' => Some(Event::Ctrl('c')),
                    '\x04' => Some(Event::Ctrl('d')),
                    c if c.is_control() => None, // Ignore other control chars
                    c => Some(Event::Char(c)),
                }
            }
            ParserState::EscapeStart => {
                match c {
                    '[' => {
                        self.state = ParserState::CsiStart;
                        None
                    }
                    _ => {
                        // Non-CSI escape sequences - return to normal and emit the character
                        self.state = ParserState::Normal;
                        Some(Event::Char(c))
                    }
                }
            }
            ParserState::CsiStart => {
                match c {
                    'A' => {
                        self.state = ParserState::Normal;
                        Some(Event::Arrow(Direction::Up))
                    }
                    'B' => {
                        self.state = ParserState::Normal;
                        Some(Event::Arrow(Direction::Down))
                    }
                    'C' => {
                        self.state = ParserState::Normal;
                        Some(Event::Arrow(Direction::Right))
                    }
                    'D' => {
                        self.state = ParserState::Normal;
                        Some(Event::Arrow(Direction::Left))
                    }
                    '3' => {
                        self.state = ParserState::CsiDelete;
                        None
                    }
                    _ => {
                        // Unknown CSI sequence, return to normal
                        self.state = ParserState::Normal;
                        None
                    }
                }
            }
            ParserState::CsiDelete => {
                self.state = ParserState::Normal;
                match c {
                    '~' => Some(Event::Delete),
                    _ => None, // Invalid delete sequence
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct WasmInputBackend {
    initialized: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    draft_line: Option<String>,
    parser: EventParser,
}

impl InputBackend for WasmInputBackend {
    async fn read_line(&mut self, _shell: &ShellRef, prompt: InteractivePrompt) -> Result<ReadResult, ShellError> {
        match self.read_line_internal(&prompt.prompt).await {
            Ok(s) => Ok(ReadResult::Input(s)),
            Err(e) if e == "EOF" => Ok(ReadResult::Eof),
            Err(e) if e == "Interrupted" => Ok(ReadResult::Interrupted),
            Err(e) => Err(ShellError::from(std::io::Error::new(std::io::ErrorKind::Other, e))),
        }
    }
}

impl WasmInputBackend {
    pub fn new() -> Self {
        Self {
            initialized: false,
            history: Vec::new(),
            history_index: None,
            draft_line: None,
            parser: EventParser::new(),
        }
    }

    async fn ensure_initialized(&mut self) -> Result<(), String> {
        if !self.initialized {
            crate::bridges::tty::ensure_bridge()?;
            self.initialized = true;
        }
        Ok(())
    }

    /// Step 1: Mode detection - try to enable raw mode, fallback to piped mode
    async fn try_enable_raw_mode(&self) -> bool {
        let fut = WasmHostFuture::new("washm_tty.enableRaw()");
        if let Ok(res_json) = fut.await {
            #[derive(Deserialize)]
            struct RawModeResult {
                ok: bool,
            }
            
            if let Ok(result) = serde_json::from_str::<RawModeResult>(&res_json) {
                return result.ok;
            }
            // On JSON parse error, assume we're in a piped/non-TTY environment
        }
        // On any error, fall back to piped mode
        false
    }

    /// Piped mode: read until newline
    async fn read_line_piped(&self) -> Result<String, String> {
        let mut line = String::new();
        
        loop {
            let fut = WasmHostFuture::new("washm_tty.read()");
            let res_json = fut.await.map_err(|e| e.to_string())?;
            
            #[derive(Deserialize)]
            struct HostResult {
                ok: bool,
                result: Option<String>,
                error: Option<String>,
            }

            let host_res: HostResult = serde_json::from_str(&res_json)
                .map_err(|e| format!("JSON error: {}", e))?;
            
            if !host_res.ok {
                return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
            }
            
            match host_res.result {
                None => {
                    if line.is_empty() {
                        return Err("EOF".to_string());
                    }
                    return Ok(line);
                }
                Some(s) => {
                    for c in s.chars() {
                        if c == '\r' || c == '\n' {
                            return Ok(line);
                        }
                        line.push(c);
                    }
                }
            }
        }
    }

    /// Render the current line state
    fn render(&self, prompt: &str, buffer: &LineBuffer) -> Result<(), String> {
        // Step 5: "Clear & Redraw Line" rendering
        // 1. Carriage return to start of line
        print!("\r");
        // 2. Print prompt + buffer
        print!("{}{}", prompt, buffer.to_string());
        // 3. Clear to end of line
        print!("\x1b[K");
        
        // 4-5. Move cursor to correct position
        // Move left from end of line if cursor is not at the end
        let chars_from_end = buffer.len().saturating_sub(buffer.cursor);
        if chars_from_end > 0 {
            print!("\x1b[{}D", chars_from_end);
        }
        
        io::stdout().flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Step 2: Fetch and parse events from raw input
    async fn fetch_events(&mut self) -> Result<Vec<Event>, String> {
        let fut = WasmHostFuture::new("washm_tty.read()");
        let res_json = fut.await.map_err(|e| e.to_string())?;
        
        #[derive(Deserialize)]
        struct HostResult {
            ok: bool,
            result: Option<String>,
            error: Option<String>,
        }

        let host_res: HostResult = serde_json::from_str(&res_json)
            .map_err(|e| format!("JSON error: {}", e))?;
        
        if !host_res.ok {
            return Err(host_res.error.unwrap_or_else(|| "Unknown host error".to_string()));
        }
        
        match host_res.result {
            None => Err("EOF".to_string()),
            Some(s) => {
                let mut events = Vec::new();
                
                for c in s.chars() {
                    if let Some(event) = self.parser.parse(c) {
                        events.push(event);
                    }
                }
                
                Ok(events)
            }
        }
    }

    /// Interactive mode: full line editing with history
    async fn read_line_interactive(&mut self, prompt: &str) -> Result<String, String> {
        let mut buffer = LineBuffer::new();
        
        // Reset history navigation state
        self.history_index = None;
        self.draft_line = None;
        
        // Initial render
        self.render(prompt, &buffer)?;
        
        loop {
            let events = self.fetch_events().await?;
            
            for event in events {
                match event {
                    Event::Char(c) => {
                        buffer.insert(c);
                        self.render(prompt, &buffer)?;
                    }
                    Event::Backspace => {
                        if buffer.backspace() {
                            self.render(prompt, &buffer)?;
                        }
                    }
                    Event::Delete => {
                        if buffer.delete() {
                            self.render(prompt, &buffer)?;
                        }
                    }
                    Event::Arrow(Direction::Left) => {
                        buffer.move_cursor(-1);
                        self.render(prompt, &buffer)?;
                    }
                    Event::Arrow(Direction::Right) => {
                        buffer.move_cursor(1);
                        self.render(prompt, &buffer)?;
                    }
                    Event::Arrow(Direction::Up) => {
                        // Step 4: History navigation
                        if self.history.is_empty() {
                            continue;
                        }
                        
                        // Save current draft if this is first history navigation
                        if self.history_index.is_none() {
                            self.draft_line = Some(buffer.to_string());
                        }
                        
                        let current_idx = self.history_index.unwrap_or(self.history.len());
                        if current_idx > 0 {
                            let new_idx = current_idx - 1;
                            self.history_index = Some(new_idx);
                            buffer = LineBuffer::from_string(self.history[new_idx].clone());
                            self.render(prompt, &buffer)?;
                        }
                    }
                    Event::Arrow(Direction::Down) => {
                        // Step 4: History navigation
                        if let Some(idx) = self.history_index {
                            if idx + 1 < self.history.len() {
                                let new_idx = idx + 1;
                                self.history_index = Some(new_idx);
                                buffer = LineBuffer::from_string(self.history[new_idx].clone());
                                self.render(prompt, &buffer)?;
                            } else {
                                // Restore draft
                                self.history_index = None;
                                if let Some(draft) = self.draft_line.take() {
                                    buffer = LineBuffer::from_string(draft);
                                } else {
                                    buffer = LineBuffer::new();
                                }
                                self.render(prompt, &buffer)?;
                            }
                        }
                    }
                    Event::Enter => {
                        println!();
                        let line = buffer.to_string();
                        
                        // Add to history if non-empty
                        if !line.trim().is_empty() {
                            self.history.push(line.clone());
                        }
                        
                        return Ok(line);
                    }
                    Event::Ctrl('c') => {
                        println!();
                        return Err("Interrupted".to_string());
                    }
                    Event::Ctrl('d') => {
                        if buffer.is_empty() {
                            println!();
                            return Err("EOF".to_string());
                        }
                    }
                    Event::Tab => {
                        // Step 6: Tab completion (placeholder for now)
                        // Future: invoke completion handler if available
                    }
                    Event::Esc => {
                        // Ignore standalone ESC for now
                    }
                    Event::Ctrl(_) => {
                        // Ignore other control characters for now
                    }
                }
            }
        }
    }

    pub async fn read_line_internal(&mut self, prompt: &str) -> Result<String, String> {
        if let Err(e) = self.ensure_initialized().await {
            return Err(e);
        }
        
        // Step 1: Mode detection
        let is_interactive = self.try_enable_raw_mode().await;
        
        if is_interactive {
            // Interactive mode with full line editing
            self.read_line_interactive(prompt).await
        } else {
            // Piped mode: simple line reading
            print!("{}", prompt);
            io::stdout().flush().map_err(|e| e.to_string())?;
            self.read_line_piped().await
        }
    }
}

// Shim for other backends
pub type ReedlineInputBackend = WasmInputBackend;
pub type BasicInputBackend = WasmInputBackend;
pub type MinimalInputBackend = WasmInputBackend;
