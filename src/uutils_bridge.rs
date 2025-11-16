/// A tiny bridge stub for `uutils` coreutils library in the root crate.
///
/// For now we only log the command name and arguments — the user may later
/// wire this to `coreutils-fork` sub-crate `uu` libraries when they want an in-process call.
pub struct UutilsBridge;

impl UutilsBridge {
    /// Run a command by name; return an exit code.
    pub fn run(name: &str, args: &[String]) -> i32 {
        println!("[uutils] running {} with args: {:?}", name, args);
        0
    }
}
