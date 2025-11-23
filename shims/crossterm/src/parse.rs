use crate::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::io;

// Internal event enum to handle things like CursorPosition which aren't in the public Event enum
#[derive(Debug, Clone, PartialEq)]
pub enum InternalEvent {
    Event(Event),
    CursorPosition(u16, u16),
}

fn could_not_parse_event_error() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "Could not parse an event.")
}

pub fn parse_event(buffer: &[u8], input_available: bool) -> io::Result<Option<InternalEvent>> {
    if buffer.is_empty() {
        return Ok(None);
    }

    match buffer[0] {
        b'\x1B' => {
            if buffer.len() == 1 {
                if input_available {
                    Ok(None) // Wait for more
                } else {
                    Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Esc.into()))))
                }
            } else {
                match buffer[1] {
                    b'O' => {
                        if buffer.len() == 2 {
                            Ok(None)
                        } else {
                            match buffer[2] {
                                b'D' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Left.into())))),
                                b'C' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Right.into())))),
                                b'A' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Up.into())))),
                                b'B' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Down.into())))),
                                b'H' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Home.into())))),
                                b'F' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::End.into())))),
                                val @ b'P'..=b'S' => Ok(Some(InternalEvent::Event(Event::Key(
                                    KeyCode::F(1 + val - b'P').into(),
                                )))),
                                _ => Err(could_not_parse_event_error()),
                            }
                        }
                    }
                    b'[' => parse_csi(buffer),
                    b'\x1B' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Esc.into())))),
                    _ => parse_event(&buffer[1..], input_available).map(|event_option| {
                        event_option.map(|event| {
                            if let InternalEvent::Event(Event::Key(mut key_event)) = event {
                                key_event.modifiers.insert(KeyModifiers::ALT);
                                InternalEvent::Event(Event::Key(key_event))
                            } else {
                                event
                            }
                        })
                    }),
                }
            }
        }
        b'\r' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Enter.into())))),
        b'\n' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Enter.into())))), // Simplified
        b'\t' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Tab.into())))),
        b'\x7F' => Ok(Some(InternalEvent::Event(Event::Key(KeyCode::Backspace.into())))),
        c @ b'\x01'..=b'\x1A' => Ok(Some(InternalEvent::Event(Event::Key(KeyEvent::new(
            KeyCode::Char((c - 0x1 + b'a') as char),
            KeyModifiers::CONTROL,
        ))))),
        c @ b'\x1C'..=b'\x1F' => Ok(Some(InternalEvent::Event(Event::Key(KeyEvent::new(
            KeyCode::Char((c - 0x1C + b'4') as char),
            KeyModifiers::CONTROL,
        ))))),
        _ => parse_utf8_char(buffer).map(|maybe_char| {
            maybe_char.map(|c| InternalEvent::Event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))))
        }),
    }
}

fn parse_csi(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    assert!(buffer.starts_with(b"\x1B[")); 

    if buffer.len() == 2 {
        return Ok(None);
    }

    let event = match buffer[2] {
        b'[' => {
            if buffer.len() == 3 {
                None
            } else {
                match buffer[3] {
                    val @ b'A'..=b'E' => Some(InternalEvent::Event(Event::Key(KeyCode::F(1 + val - b'A').into()))),
                    _ => return Err(could_not_parse_event_error()),
                }
            }
        }
        b'D' => Some(InternalEvent::Event(Event::Key(KeyCode::Left.into()))),
        b'C' => Some(InternalEvent::Event(Event::Key(KeyCode::Right.into()))),
        b'A' => Some(InternalEvent::Event(Event::Key(KeyCode::Up.into()))),
        b'B' => Some(InternalEvent::Event(Event::Key(KeyCode::Down.into()))),
        b'H' => Some(InternalEvent::Event(Event::Key(KeyCode::Home.into()))),
        b'F' => Some(InternalEvent::Event(Event::Key(KeyCode::End.into()))),
        b'Z' => Some(InternalEvent::Event(Event::Key(KeyEvent::new_with_kind(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
            KeyEventKind::Press,
        )))),
        b'M' => return parse_csi_normal_mouse(buffer),
        b'<' => return parse_csi_sgr_mouse(buffer),
        b';' => return parse_csi_modifier_key_code(buffer),
        b'0'..=b'9' => {
            if buffer.len() == 3 {
                return Ok(None);
            }
            let last_byte = buffer[buffer.len() - 1];
            if !(64..=126).contains(&last_byte) {
                return Ok(None);
            }
            
            if buffer.starts_with(b"\x1B[200~") {
                 return parse_csi_bracketed_paste(buffer);
            }

            match last_byte {
                b'M' => return parse_csi_rxvt_mouse(buffer),
                b'~' => return parse_csi_special_key_code(buffer),
                b'R' => return parse_csi_cursor_position(buffer),
                _ => return parse_csi_modifier_key_code(buffer),
            }
        }
        _ => return Err(could_not_parse_event_error()),
    };
    
    Ok(event)
}

fn parse_csi_modifier_key_code(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    let s = std::str::from_utf8(&buffer[2..buffer.len() - 1])
        .map_err(|_| could_not_parse_event_error())?;
    let mut split = s.split(';');
    
    // We ignore the first number (1) usually
    let _ = split.next(); 
    
    let modifiers = if let Some(mod_str) = split.next() {
        let mod_val = mod_str.parse::<u8>().map_err(|_| could_not_parse_event_error())?;
        parse_modifiers(mod_val)
    } else {
        KeyModifiers::NONE
    };

    let key = buffer[buffer.len() - 1];
    let keycode = match key {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'F' => KeyCode::End,
        b'H' => KeyCode::Home,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        _ => return Err(could_not_parse_event_error()),
    };

    Ok(Some(InternalEvent::Event(Event::Key(KeyEvent::new(keycode, modifiers)))))
}

fn parse_csi_special_key_code(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    let s = std::str::from_utf8(&buffer[2..buffer.len() - 1])
        .map_err(|_| could_not_parse_event_error())?;
    let mut split = s.split(';');
    
    let first = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u8>().map_err(|_| could_not_parse_event_error())?;
    
    let modifiers = if let Some(mod_str) = split.next() {
        let mod_val = mod_str.parse::<u8>().map_err(|_| could_not_parse_event_error())?;
        parse_modifiers(mod_val)
    } else {
        KeyModifiers::NONE
    };

    let keycode = match first {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        v @ 11..=15 => KeyCode::F(v - 10),
        v @ 17..=21 => KeyCode::F(v - 11),
        v @ 23..=26 => KeyCode::F(v - 12),
        v @ 28..=29 => KeyCode::F(v - 15),
        v @ 31..=34 => KeyCode::F(v - 17),
        _ => return Err(could_not_parse_event_error()),
    };

    Ok(Some(InternalEvent::Event(Event::Key(KeyEvent::new(keycode, modifiers)))))
}

fn parse_csi_cursor_position(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    let s = std::str::from_utf8(&buffer[2..buffer.len() - 1])
        .map_err(|_| could_not_parse_event_error())?;
    let mut split = s.split(';');
    
    let row = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    let col = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    
    Ok(Some(InternalEvent::CursorPosition(col.saturating_sub(1), row.saturating_sub(1))))
}

fn parse_csi_normal_mouse(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    // ESC [ M Cb Cx Cy
    if buffer.len() < 6 {
        return Ok(None);
    }
    
    let cb = buffer[3];
    let cx = buffer[4] as u16;
    let cy = buffer[5] as u16;
    
    let (kind, modifiers) = parse_cb(cb.saturating_sub(32))?;
    
    Ok(Some(InternalEvent::Event(Event::Mouse(MouseEvent {
        kind,
        column: cx.saturating_sub(32).saturating_sub(1),
        row: cy.saturating_sub(32).saturating_sub(1),
        modifiers,
    }))))
}

fn parse_csi_sgr_mouse(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    // ESC [ < Cb ; Cx ; Cy (;) (M or m)
    if !buffer.ends_with(b"m") && !buffer.ends_with(b"M") {
        return Ok(None);
    }
    
    let s = std::str::from_utf8(&buffer[3..buffer.len() - 1])
        .map_err(|_| could_not_parse_event_error())?;
    let mut split = s.split(';');
    
    let cb = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u8>().map_err(|_| could_not_parse_event_error())?;
    let cx = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    let cy = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    
    let (mut kind, modifiers) = parse_cb(cb)?;
    
    if buffer.last() == Some(&b'm') {
        if let MouseEventKind::Down(btn) = kind {
            kind = MouseEventKind::Up(btn);
        }
    }
    
    Ok(Some(InternalEvent::Event(Event::Mouse(MouseEvent {
        kind,
        column: cx.saturating_sub(1),
        row: cy.saturating_sub(1),
        modifiers,
    }))))
}

fn parse_csi_rxvt_mouse(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    // ESC [ Cb ; Cx ; Cy ; M
    let s = std::str::from_utf8(&buffer[2..buffer.len() - 1])
        .map_err(|_| could_not_parse_event_error())?;
    let mut split = s.split(';');
    
    let cb = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u8>().map_err(|_| could_not_parse_event_error())?;
    let cx = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    let cy = split.next().ok_or_else(could_not_parse_event_error)?.parse::<u16>().map_err(|_| could_not_parse_event_error())?;
    
    let (kind, modifiers) = parse_cb(cb.saturating_sub(32))?;
    
    Ok(Some(InternalEvent::Event(Event::Mouse(MouseEvent {
        kind,
        column: cx.saturating_sub(1),
        row: cy.saturating_sub(1),
        modifiers,
    }))))
}

fn parse_csi_bracketed_paste(buffer: &[u8]) -> io::Result<Option<InternalEvent>> {
    if !buffer.ends_with(b"\x1b[201~") {
        return Ok(None);
    }
    let paste = String::from_utf8_lossy(&buffer[6..buffer.len() - 6]).to_string();
    Ok(Some(InternalEvent::Event(Event::Paste(paste))))
}

fn parse_utf8_char(buffer: &[u8]) -> io::Result<Option<char>> {
    match std::str::from_utf8(buffer) {
        Ok(s) => Ok(s.chars().next()),
        Err(_) => {
            let required = match buffer[0] {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => return Err(could_not_parse_event_error()),
            };
            if buffer.len() < required {
                Ok(None)
            } else {
                Err(could_not_parse_event_error())
            }
        }
    }
}

fn parse_modifiers(mask: u8) -> KeyModifiers {
    let mut m = KeyModifiers::NONE;
    if mask & 1 != 0 { m |= KeyModifiers::SHIFT; }
    if mask & 2 != 0 { m |= KeyModifiers::ALT; }
    if mask & 4 != 0 { m |= KeyModifiers::CONTROL; }
    m
}

fn parse_cb(cb: u8) -> io::Result<(MouseEventKind, KeyModifiers)> {
    let mut modifiers = KeyModifiers::NONE;
    if cb & 4 != 0 { modifiers |= KeyModifiers::SHIFT; }
    if cb & 8 != 0 { modifiers |= KeyModifiers::ALT; }
    if cb & 16 != 0 { modifiers |= KeyModifiers::CONTROL; }
    
    let kind = match cb & 3 {
        0 => if cb & 64 != 0 { MouseEventKind::ScrollUp } else { MouseEventKind::Down(MouseButton::Left) },
        1 => if cb & 64 != 0 { MouseEventKind::ScrollDown } else { MouseEventKind::Down(MouseButton::Middle) },
        2 => MouseEventKind::Down(MouseButton::Right),
        3 => MouseEventKind::Up(MouseButton::Left), // Approximate
        _ => MouseEventKind::Moved,
    };
    
    Ok((kind, modifiers))
}

pub struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn advance(&mut self, buffer: &[u8], more: bool) -> Vec<InternalEvent> {
        let mut events = Vec::new();
        for byte in buffer {
            self.buffer.push(*byte);
            
            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    events.push(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    // Need more bytes
                }
                Err(_) => {
                    // Error, clear buffer
                    self.buffer.clear();
                }
            }
        }
        events
    }
}
