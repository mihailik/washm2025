
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitStatus(i32);

impl ExitStatus {
    pub fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn success(&self) -> bool {
        self.0 == 0
    }

    pub fn code(&self) -> Option<i32> {
        Some(self.0)
    }
}

impl Default for ExitStatus {
    fn default() -> Self {
        Self(0)
    }
}

pub trait ExitStatusExt {
    fn from_raw(raw: i32) -> Self;
    fn signal(&self) -> Option<i32>;
    fn core_dumped(&self) -> bool;
    fn stopped(&self) -> bool;
    fn continued(&self) -> bool;
    fn into_raw(&self) -> i32;
}

impl ExitStatusExt for ExitStatus {
    fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    fn signal(&self) -> Option<i32> {
        None // TODO: Implement signal handling if needed
    }

    fn core_dumped(&self) -> bool {
        false
    }

    fn stopped(&self) -> bool {
        false
    }

    fn continued(&self) -> bool {
        false
    }

    fn into_raw(&self) -> i32 {
        self.0
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
