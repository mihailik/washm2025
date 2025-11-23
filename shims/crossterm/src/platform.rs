use std::sync::OnceLock;
use parking_lot::RwLock;
use std::io;

pub struct TtyDelegates {
    pub enable_raw_mode: Box<dyn Fn() -> io::Result<()> + Send + Sync + 'static>,
    pub disable_raw_mode: Box<dyn Fn() -> io::Result<()> + Send + Sync + 'static>,
    pub size: Box<dyn Fn() -> io::Result<(u16, u16)> + Send + Sync + 'static>,
}

impl Default for TtyDelegates {
    fn default() -> Self {
        Self {
            enable_raw_mode: Box::new(|| Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))),
            disable_raw_mode: Box::new(|| Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))),
            size: Box::new(|| Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))),
        }
    }
}

static TTY_DELEGATES: OnceLock<RwLock<Option<TtyDelegates>>> = OnceLock::new();

fn delegates() -> &'static RwLock<Option<TtyDelegates>> {
    TTY_DELEGATES.get_or_init(|| RwLock::new(None))
}

pub fn set_tty_delegates(d: TtyDelegates) {
    let lock = delegates();
    let mut guard = lock.write();
    *guard = Some(d);
}

pub fn enable_raw_mode() -> io::Result<()> {
    if let Some(ref d) = *delegates().read() {
        (d.enable_raw_mode)()
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))
    }
}

pub fn disable_raw_mode() -> io::Result<()> {
    if let Some(ref d) = *delegates().read() {
        (d.disable_raw_mode)()
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))
    }
}

pub fn size() -> io::Result<(u16, u16)> {
    if let Some(ref d) = *delegates().read() {
        (d.size)()
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "tty delegate not set"))
    }
}
