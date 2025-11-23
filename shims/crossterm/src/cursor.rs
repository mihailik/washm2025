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
