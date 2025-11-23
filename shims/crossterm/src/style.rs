use crate::Command;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Reset,
    Black,
    DarkGrey,
    Red,
    DarkRed,
    Green,
    DarkGreen,
    Yellow,
    DarkYellow,
    Blue,
    DarkBlue,
    Magenta,
    DarkMagenta,
    Cyan,
    DarkCyan,
    White,
    Grey,
    Rgb { r: u8, g: u8, b: u8 },
    AnsiValue(u8),
}

impl Color {
    fn write_fg(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self {
            Color::Reset => write!(f, "\x1b[39m"),
            Color::Black => write!(f, "\x1b[30m"),
            Color::DarkGrey => write!(f, "\x1b[90m"),
            Color::Red => write!(f, "\x1b[91m"),
            Color::DarkRed => write!(f, "\x1b[31m"),
            Color::Green => write!(f, "\x1b[92m"),
            Color::DarkGreen => write!(f, "\x1b[32m"),
            Color::Yellow => write!(f, "\x1b[93m"),
            Color::DarkYellow => write!(f, "\x1b[33m"),
            Color::Blue => write!(f, "\x1b[94m"),
            Color::DarkBlue => write!(f, "\x1b[34m"),
            Color::Magenta => write!(f, "\x1b[95m"),
            Color::DarkMagenta => write!(f, "\x1b[35m"),
            Color::Cyan => write!(f, "\x1b[96m"),
            Color::DarkCyan => write!(f, "\x1b[36m"),
            Color::White => write!(f, "\x1b[97m"),
            Color::Grey => write!(f, "\x1b[37m"),
            Color::Rgb { r, g, b } => write!(f, "\x1b[38;2;{};{};{}m", r, g, b),
            Color::AnsiValue(v) => write!(f, "\x1b[38;5;{}m", v),
        }
    }

    fn write_bg(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self {
            Color::Reset => write!(f, "\x1b[49m"),
            Color::Black => write!(f, "\x1b[40m"),
            Color::DarkGrey => write!(f, "\x1b[100m"),
            Color::Red => write!(f, "\x1b[101m"),
            Color::DarkRed => write!(f, "\x1b[41m"),
            Color::Green => write!(f, "\x1b[102m"),
            Color::DarkGreen => write!(f, "\x1b[42m"),
            Color::Yellow => write!(f, "\x1b[103m"),
            Color::DarkYellow => write!(f, "\x1b[43m"),
            Color::Blue => write!(f, "\x1b[104m"),
            Color::DarkBlue => write!(f, "\x1b[44m"),
            Color::Magenta => write!(f, "\x1b[105m"),
            Color::DarkMagenta => write!(f, "\x1b[45m"),
            Color::Cyan => write!(f, "\x1b[106m"),
            Color::DarkCyan => write!(f, "\x1b[46m"),
            Color::White => write!(f, "\x1b[107m"),
            Color::Grey => write!(f, "\x1b[47m"),
            Color::Rgb { r, g, b } => write!(f, "\x1b[48;2;{};{};{}m", r, g, b),
            Color::AnsiValue(v) => write!(f, "\x1b[48;5;{}m", v),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribute {
    Reset,
    Bold,
    Dim,
    Italic,
    Underlined,
    SlowBlink,
    RapidBlink,
    Reverse,
    Hidden,
    CrossedOut,
    NoBold,
    NormalIntensity,
    NoItalic,
    NoUnderline,
    NoBlink,
    NoReverse,
    NoHidden,
    NotCrossedOut,
}

impl Attribute {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        match self {
            Attribute::Reset => write!(f, "\x1b[0m"),
            Attribute::Bold => write!(f, "\x1b[1m"),
            Attribute::Dim => write!(f, "\x1b[2m"),
            Attribute::Italic => write!(f, "\x1b[3m"),
            Attribute::Underlined => write!(f, "\x1b[4m"),
            Attribute::SlowBlink => write!(f, "\x1b[5m"),
            Attribute::RapidBlink => write!(f, "\x1b[6m"),
            Attribute::Reverse => write!(f, "\x1b[7m"),
            Attribute::Hidden => write!(f, "\x1b[8m"),
            Attribute::CrossedOut => write!(f, "\x1b[9m"),
            Attribute::NoBold => write!(f, "\x1b[22m"),
            Attribute::NormalIntensity => write!(f, "\x1b[22m"),
            Attribute::NoItalic => write!(f, "\x1b[23m"),
            Attribute::NoUnderline => write!(f, "\x1b[24m"),
            Attribute::NoBlink => write!(f, "\x1b[25m"),
            Attribute::NoReverse => write!(f, "\x1b[27m"),
            Attribute::NoHidden => write!(f, "\x1b[28m"),
            Attribute::NotCrossedOut => write!(f, "\x1b[29m"),
        }
    }
}

pub struct SetForegroundColor(pub Color);
impl Command for SetForegroundColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        self.0.write_fg(f)
    }
}

pub struct SetBackgroundColor(pub Color);
impl Command for SetBackgroundColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        self.0.write_bg(f)
    }
}

pub struct SetAttribute(pub Attribute);
impl Command for SetAttribute {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        self.0.write_ansi(f)
    }
}

pub struct ResetColor;
impl Command for ResetColor {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[0m")
    }
}

pub struct Print<T: fmt::Display>(pub T);
impl<T: fmt::Display> Command for Print<T> {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait Stylize: Sized {
    type Styled;
    fn with(self, color: Color) -> Self::Styled;
    fn on(self, color: Color) -> Self::Styled;
    fn attribute(self, attr: Attribute) -> Self::Styled;
    
    // Convenience methods
    fn black(self) -> Self::Styled { self.with(Color::Black) }
    fn red(self) -> Self::Styled { self.with(Color::Red) }
    fn green(self) -> Self::Styled { self.with(Color::Green) }
    fn yellow(self) -> Self::Styled { self.with(Color::Yellow) }
    fn blue(self) -> Self::Styled { self.with(Color::Blue) }
    fn magenta(self) -> Self::Styled { self.with(Color::Magenta) }
    fn cyan(self) -> Self::Styled { self.with(Color::Cyan) }
    fn white(self) -> Self::Styled { self.with(Color::White) }
}

// Minimal implementation for Stylize on &str and String
// This is usually more complex in crossterm (StyledContent), but for now we can stub it or implement it.
// Reedline uses Stylize heavily.

pub struct StyledContent<D: fmt::Display> {
    content: D,
    fg: Option<Color>,
    bg: Option<Color>,
    attrs: Vec<Attribute>,
}

impl<D: fmt::Display> fmt::Display for StyledContent<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(fg) = self.fg {
            let mut s = String::new();
            fg.write_fg(&mut s).unwrap();
            write!(f, "{}", s)?;
        }
        if let Some(bg) = self.bg {
            let mut s = String::new();
            bg.write_bg(&mut s).unwrap();
            write!(f, "{}", s)?;
        }
        for attr in &self.attrs {
            let mut s = String::new();
            attr.write_ansi(&mut s).unwrap();
            write!(f, "{}", s)?;
        }
        write!(f, "{}", self.content)?;
        if self.fg.is_some() || self.bg.is_some() || !self.attrs.is_empty() {
            write!(f, "\x1b[0m")?;
        }
        Ok(())
    }
}

macro_rules! impl_stylize {
    ($($t:ty),*) => {
        $(
            impl Stylize for $t {
                type Styled = StyledContent<$t>;
                fn with(self, color: Color) -> Self::Styled {
                    StyledContent { content: self, fg: Some(color), bg: None, attrs: Vec::new() }
                }
                fn on(self, color: Color) -> Self::Styled {
                    StyledContent { content: self, fg: None, bg: Some(color), attrs: Vec::new() }
                }
                fn attribute(self, attr: Attribute) -> Self::Styled {
                    StyledContent { content: self, fg: None, bg: None, attrs: vec![attr] }
                }
            }
        )*
    };
}

impl_stylize!(String, char);

impl<'a> Stylize for &'a str {
    type Styled = StyledContent<&'a str>;
    fn with(self, color: Color) -> Self::Styled {
        StyledContent { content: self, fg: Some(color), bg: None, attrs: Vec::new() }
    }
    fn on(self, color: Color) -> Self::Styled {
        StyledContent { content: self, fg: None, bg: Some(color), attrs: Vec::new() }
    }
    fn attribute(self, attr: Attribute) -> Self::Styled {
        StyledContent { content: self, fg: None, bg: None, attrs: vec![attr] }
    }
}

impl<D: fmt::Display + Clone> Stylize for &D {
    type Styled = StyledContent<D>;
    fn with(self, color: Color) -> Self::Styled {
        StyledContent { content: self.clone(), fg: Some(color), bg: None, attrs: Vec::new() }
    }
    fn on(self, color: Color) -> Self::Styled {
        StyledContent { content: self.clone(), fg: None, bg: Some(color), attrs: Vec::new() }
    }
    fn attribute(self, attr: Attribute) -> Self::Styled {
        StyledContent { content: self.clone(), fg: None, bg: None, attrs: vec![attr] }
    }
}

impl<D: fmt::Display> Stylize for StyledContent<D> {
    type Styled = StyledContent<D>;
    fn with(mut self, color: Color) -> Self::Styled {
        self.fg = Some(color);
        self
    }
    fn on(mut self, color: Color) -> Self::Styled {
        self.bg = Some(color);
        self
    }
    fn attribute(mut self, attr: Attribute) -> Self::Styled {
        self.attrs.push(attr);
        self
    }
}
