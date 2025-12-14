use crate::brush_core::builtins::SimpleCommand;
use crate::brush_core::{commands, results, error};

macro_rules! make_coreutil {
    ($name:ident, $cmd:expr, $desc:expr) => {
        pub struct $name;

        impl SimpleCommand for $name {
            fn get_content(name: &str, _content_type: crate::brush_core::builtins::ContentType, _options: &crate::brush_core::builtins::ContentOptions) -> Result<String, error::Error> {
                Ok(format!("{} - {}", name, $desc))
            }

            fn execute<I: Iterator<Item = S>, S: AsRef<str>>( _context: commands::ExecutionContext<'_>, args: I,) -> Result<results::ExecutionResult, error::Error> {
                let args_vec: Vec<String> = args.map(|s| s.as_ref().to_string()).collect();
                let code = crate::uutils_bridge::UutilsBridge::run($cmd, &args_vec);
                Ok(results::ExecutionResult::new(code as u8))
            }
        }
    };
}

// Common coreutils wrappers
make_coreutil!(LsCommand, "ls", "list directory contents");
make_coreutil!(CpCommand, "cp", "copy files and directories");
make_coreutil!(MvCommand, "mv", "move/rename files");
make_coreutil!(RmCommand, "rm", "remove files or directories");
make_coreutil!(MkdirCommand, "mkdir", "make directories");
make_coreutil!(RmdirCommand, "rmdir", "remove empty directories");
make_coreutil!(TouchCommand, "touch", "change file timestamps or create files");
