// Minimal patch module implementing a subset of Crossterm API so our project
// and dependencies compile for WASM targets. It intentionally provides no-op
// implementations or stubs for platform-specific functionality.
pub mod platform {
    pub use super::platform_shim::{TtyDelegates, set_tty_delegates};
}

mod platform_shim {
    use std::io;
    use std::sync::Arc;

    pub struct TtyDelegates {
        pub enable_raw_mode: Arc<dyn Fn() -> io::Result<()> + Send + Sync>,
        pub disable_raw_mode: Arc<dyn Fn() -> io::Result<()> + Send + Sync>,
        pub size: Arc<dyn Fn() -> io::Result<(u16, u16)> + Send + Sync>,
    }

    use once_cell::sync::OnceCell;
    use std::sync::Mutex;
    static DELEGATES: OnceCell<Mutex<Option<TtyDelegates>>> = OnceCell::new();

    pub fn set_tty_delegates(d: TtyDelegates) {
        let cell = DELEGATES.get_or_init(|| Mutex::new(None));
        let mut g = cell.lock().unwrap();
        *g = Some(d);
    }
}

pub mod event {
    use std::io;
    #[derive(Clone, Copy, Debug)]
    pub enum KeyModifiers { NONE, SHIFT, CONTROL }
    #[derive(Clone, Copy, Debug)]
    pub enum KeyCode { Char(char), Enter, Backspace, Left, Tab }
    #[derive(Clone, Copy, Debug)]
    pub struct KeyEvent { pub modifiers: KeyModifiers, pub code: KeyCode }
    #[derive(Clone, Copy, Debug)]
    pub enum Event { Key(KeyEvent) }
    pub fn read() -> io::Result<Event> { Err(io::Error::new(io::ErrorKind::Other, "not supported")) }
}

pub mod terminal { pub use super::terminal_shim::{Clear, ClearType, size}; }
mod terminal_shim {
    use std::io;
    #[derive(Clone, Copy, Debug)] pub enum ClearType { All }
    pub struct Clear(pub ClearType);
    pub fn size() -> io::Result<(u16, u16)> { Err(io::Error::new(io::ErrorKind::Other, "not supported")) }
}

pub mod cursor { pub use super::cursor_shim::MoveTo; }
mod cursor_shim { #[derive(Clone, Copy, Debug)] pub struct MoveTo(pub u16, pub u16); }

pub use std::io::Result;

pub trait Command { fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result; }
pub trait ExecutableCommand { fn execute<C: Command>(&mut self, command: C) -> Result<&mut Self>; }
pub trait QueueableCommand { fn queue<C: Command>(&mut self, command: C) -> Result<&mut Self>; }

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
macro_rules! execute { ($writer:expr, $($command:expr),* $(,)?) => {{ use $crate::crossterm_shim::ExecutableCommand; let mut res = Ok(&mut $writer); $( if res.is_ok() { res = res.unwrap().execute($command); } )* res.map(|_| ()) }} }
#[macro_export]
macro_rules! queue { ($writer:expr, $($command:expr),* $(,)?) => {{ use $crate::crossterm_shim::QueueableCommand; let mut res = Ok(&mut $writer); $( if res.is_ok() { res = res.unwrap().queue($command); } )* res.map(|_| ()) }} }
