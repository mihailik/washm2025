// Replacement for blanket impl
macro_rules! impl_from_error {
    ($t:ty) => {
        impl From<$t> for Error {
            fn from(err: $t) -> Self {
                Self {
                    kind: ErrorKind::from(err),
                    fatal: false,
                }
            }
        }
    }
}

impl_from_error!(std::io::Error);
impl_from_error!(std::num::TryFromIntError);
impl_from_error!(std::string::FromUtf8Error);
impl_from_error!(std::str::Utf8Error);
impl_from_error!(fancy_regex::Error);
impl_from_error!(std::fmt::Error);
impl_from_error!(tokio::task::JoinError);
impl_from_error!(std::time::SystemTimeError);
impl_from_error!(crate::brush_core::sys::PlatformError);
impl_from_error!(crate::brush_core::arithmetic::EvalError);
impl_from_error!(crate::brush_core::parser::WordParseError);
impl_from_error!(crate::brush_core::parser::TestCommandParseError);
impl_from_error!(crate::brush_core::parser::BindingParseError);
impl_from_error!(ErrorKind);
