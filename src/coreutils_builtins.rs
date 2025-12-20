use crate::brush_core::builtins::SimpleCommand;
use crate::brush_core::{commands, results, error};

macro_rules! make_coreutil {
    ($name:ident, $cmd:expr, $desc:expr) => {
        pub struct $name;

        impl SimpleCommand for $name {
            fn get_content(name: &str, _content_type: crate::brush_core::builtins::ContentType, _options: &crate::brush_core::builtins::ContentOptions) -> Result<String, error::Error> {
                Ok(format!("{} - {}", name, $desc))
            }

            fn execute<I: Iterator<Item = S>, S: AsRef<str>>( context: commands::ExecutionContext<'_>, args: I,) -> Result<results::ExecutionResult, error::Error> {
                let args_vec: Vec<String> = args.map(|s| s.as_ref().to_string()).collect();

                // Sync exported environment variables
                let current_env: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
                for k in current_env {
                    if context.shell.env.get(&k).map_or(true, |(_, v)| !v.is_exported()) {
                        std::env::remove_var(k);
                    }
                }
                for (k, v) in context.shell.env.iter_exported() {
                    if v.value().is_set() {
                        std::env::set_var(k, v.value().to_cow_str(context.shell).as_ref());
                    }
                }

                let code = crate::coreutils_dispatch::CoreutilsDispatch::run(
                    $cmd,
                    &args_vec,
                    context.stdin(),
                    context.stdout(),
                    context.stderr(),
                );
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
make_coreutil!(HeadCommand, "head", "output the first part of files");
make_coreutil!(EnvCommand, "env", "run a program in a modified environment");

// Cat is a common coreutils wrapper. Centralized here instead of being duplicated
// in various integration stubs (e.g., `brush_bridge`).
make_coreutil!(CatCommand, "cat", "concatenate files and print on the standard output");

pub struct PwdCommand;

impl SimpleCommand for PwdCommand {
    fn get_content(name: &str, _content_type: crate::brush_core::builtins::ContentType, _options: &crate::brush_core::builtins::ContentOptions) -> Result<String, error::Error> {
        Ok(format!("{} - print name of current/working directory", name))
    }

    fn execute<I: Iterator<Item = S>, S: AsRef<str>>(
        context: commands::ExecutionContext<'_>,
        _args: I,
    ) -> Result<results::ExecutionResult, error::Error> {
        // Sync exported environment variables
        let current_env: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
        for k in current_env {
            if context.shell.env.get(&k).map_or(true, |(_, v)| !v.is_exported()) {
                std::env::remove_var(k);
            }
        }
        for (k, v) in context.shell.env.iter_exported() {
            if v.value().is_set() {
                std::env::set_var(k, v.value().to_cow_str(context.shell).as_ref());
            }
        }

        // Read host start path
        let start_pwd = crate::bridges::host::startup_pwd();
        let start_pwd = start_pwd.trim();

        // Get shell's current working directory (relative to WASI root)
        let shell_cwd = context.shell.working_dir();
        let shell_cwd_str = shell_cwd.to_string_lossy();

        // Combine
        let final_path = if shell_cwd_str.starts_with(start_pwd) {
            std::path::PathBuf::from(shell_cwd_str.as_ref())
        } else if shell_cwd_str == "/" {
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
