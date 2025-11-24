use crate::Command;
use std::fmt;

pub struct MoveTo(pub u16, pub u16);

impl Command for MoveTo {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}H", self.1 + 1, self.0 + 1)
    }
}

pub struct MoveToColumn(pub u16);
impl Command for MoveToColumn {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}G", self.0 + 1)
    }
}

pub struct MoveToRow(pub u16);
impl Command for MoveToRow {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}d", self.0 + 1)
    }
}

pub struct MoveUp(pub u16);
impl Command for MoveUp {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}A", self.0)
    }
}

pub struct MoveDown(pub u16);
impl Command for MoveDown {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}B", self.0)
    }
}

pub struct MoveLeft(pub u16);
impl Command for MoveLeft {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}D", self.0)
    }
}

pub struct MoveRight(pub u16);
impl Command for MoveRight {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{}C", self.0)
    }
}

pub struct Hide;
impl Command for Hide {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?25l")
    }
}

pub struct Show;
impl Command for Show {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?25h")
    }
}

pub struct SavePosition;
impl Command for SavePosition {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[s")
    }
}

pub struct RestorePosition;
impl Command for RestorePosition {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[u")
    }
}

pub fn position() -> std::io::Result<(u16, u16)> {
    // Return a default position (0,0) for environments that cannot query the cursor.
    Ok((0, 0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetCursorStyle(pub CursorShape);
impl SetCursorStyle {
    pub const DefaultUserShape: SetCursorStyle = SetCursorStyle(CursorShape::Default);
    pub const BlinkingBlock: SetCursorStyle = SetCursorStyle(CursorShape::BlinkingBlock);
    pub const SteadyBlock: SetCursorStyle = SetCursorStyle(CursorShape::SteadyBlock);
    pub const BlinkingUnderline: SetCursorStyle = SetCursorStyle(CursorShape::BlinkingUnderline);
    pub const SteadyUnderline: SetCursorStyle = SetCursorStyle(CursorShape::SteadyUnderline);
    pub const BlinkingBar: SetCursorStyle = SetCursorStyle(CursorShape::BlinkingBar);
    pub const SteadyBar: SetCursorStyle = SetCursorStyle(CursorShape::SteadyBar);
}
impl Command for SetCursorStyle {
    fn write_ansi(&self, _f: &mut impl fmt::Write) -> fmt::Result {
        // No-op in the wasm shim. Some terminals support setting styles via CSI sequences.
        Ok(())
    }
}
