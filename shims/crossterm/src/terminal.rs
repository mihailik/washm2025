use crate::Command;
use std::fmt;
use crate::platform as platform;

pub fn enable_raw_mode() -> std::io::Result<()> {
    platform::enable_raw_mode()
}

pub fn disable_raw_mode() -> std::io::Result<()> {
    platform::disable_raw_mode()
}

pub fn size() -> std::io::Result<(u16, u16)> {
    platform::size()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearType {
    All,
    Purge,
    FromCursorDown,
    FromCursorUp,
    CurrentLine,
    UntilNewLine,
}

pub struct Clear(pub ClearType);

impl Command for Clear {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self.0 {
            ClearType::All => write!(f, "\x1b[2J"),
            ClearType::Purge => write!(f, "\x1b[3J"),
            ClearType::FromCursorDown => write!(f, "\x1b[J"),
            ClearType::FromCursorUp => write!(f, "\x1b[1J"),
            ClearType::CurrentLine => write!(f, "\x1b[2K"),
            ClearType::UntilNewLine => write!(f, "\x1b[K"),
        }
    }
}

pub struct EnterAlternateScreen;
impl Command for EnterAlternateScreen {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1049h")
    }
}

pub struct LeaveAlternateScreen;
impl Command for LeaveAlternateScreen {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1049l")
    }
}
