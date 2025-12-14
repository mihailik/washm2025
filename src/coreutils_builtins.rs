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

pub struct PwdCommand;

impl SimpleCommand for PwdCommand {
    fn get_content(name: &str, _content_type: crate::brush_core::builtins::ContentType, _options: &crate::brush_core::builtins::ContentOptions) -> Result<String, error::Error> {
        Ok(format!("{} - print name of current/working directory", name))
    }

    fn execute<I: Iterator<Item = S>, S: AsRef<str>>(
        context: commands::ExecutionContext<'_>,
        _args: I,
    ) -> Result<results::ExecutionResult, error::Error> {
        // Read host start path
        let start_pwd = std::fs::read_to_string("/dev/host/washm/start_pwd")
            .unwrap_or_else(|_| String::from("/"));
        let start_pwd = start_pwd.trim();

        // Get shell's current working directory (relative to WASI root)
        let shell_cwd = context.shell.working_dir();
        let shell_cwd_str = shell_cwd.to_string_lossy();

        // Combine
        let final_path = if shell_cwd_str == "/" {
            std::path::PathBuf::from(start_pwd)
        } else {
            let relative = shell_cwd_str.trim_start_matches('/');
            std::path::Path::new(start_pwd).join(relative)
        };

        use std::io::Write;
        writeln!(context.stdout(), "{}", final_path.display())?;
        Ok(results::ExecutionResult::success())
    }
}
