pub mod terminal;
pub mod event;
pub mod cursor;
pub mod style;
pub(crate) mod parse;
pub mod platform;

pub use std::io::Result;

pub trait Command {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result;
}

pub trait ExecutableCommand {
    fn execute<C: Command>(&mut self, command: C) -> Result<&mut Self>;
}

pub trait QueueableCommand {
    fn queue<C: Command>(&mut self, command: C) -> Result<&mut Self>;
}

impl<W: std::io::Write + ?Sized> ExecutableCommand for W {
    fn execute<C: Command>(&mut self, command: C) -> Result<&mut Self> {
        let mut buf = String::new();
        command.write_ansi(&mut buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.write_all(buf.as_bytes())?;
        self.flush()?;
        Ok(self)
    }
}

impl<W: std::io::Write + ?Sized> QueueableCommand for W {
    fn queue<C: Command>(&mut self, command: C) -> Result<&mut Self> {
        let mut buf = String::new();
        command.write_ansi(&mut buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.write_all(buf.as_bytes())?;
        Ok(self)
    }
}

#[macro_export]
macro_rules! execute {
    ($writer:expr, $($command:expr),* $(,)?) => {{
        use $crate::ExecutableCommand;
        let mut res = Ok(&mut $writer);
        $(
            if res.is_ok() {
                res = res.unwrap().execute($command);
            }
        )*
        res.map(|_| ())
    }}
}

#[macro_export]
macro_rules! queue {
    ($writer:expr, $($command:expr),* $(,)?) => {{
        use $crate::QueueableCommand;
        let mut res = Ok(&mut $writer);
        $(
            if res.is_ok() {
                res = res.unwrap().queue($command);
            }
        )*
        res.map(|_| ())
    }}
}
