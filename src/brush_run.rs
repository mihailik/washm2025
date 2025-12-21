
pub fn run(_args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    // Try to get terminal size to verify TTY integration
    if let Ok((_cols, _rows)) = crate::bridges::tty::size() {
        // TTY is available
    }

    // Get arguments to pass to brush
    let raw_args: Vec<String> = std::env::args().collect();
    
    // Skip host args (executable name, possibly washm.js and wasm file)
    let mut start_index = 1;
    if raw_args.len() > start_index && raw_args[start_index].ends_with(".js") {
        start_index += 1;
    }
    if raw_args.len() > start_index && raw_args[start_index].ends_with(".wasm") {
        start_index += 1;
    }

    // Collect user arguments
    let user_args: Vec<String> = raw_args.iter().skip(start_index).cloned().collect();
    
    // Change to the host's starting directory
    let start_pwd = crate::bridges::host::startup_pwd();
    if !start_pwd.is_empty() {
        let _ = std::env::set_current_dir(start_pwd);
    }

    // Build arguments for brush
    let mut args = Vec::new();
    args.push("brush".to_string());

    // We need to force the Minimal backend because the default Reedline backend
    // requires a TTY, which is not available in the WASI environment (piped stdin).
    args.push("--input-backend".to_string());
    args.push("minimal".to_string());
    
    // If we have user args, pass them through to brush
    // Only wrap with -c if they are NOT brush command-line options
    if !user_args.is_empty() {
        // Check if user provided -c/--command explicitly, or if the args are brush options
        let has_command_flag = user_args.iter().any(|a| a == "-c" || a == "--command");
        let has_brush_option = user_args.iter().any(|a| {
            // Check for long options or short options (but not negative numbers)
            a.starts_with("--") || (a.starts_with("-") && a.len() == 2 && a.chars().nth(1).map_or(false, |c| c.is_alphabetic()))
        });
        
        if has_command_flag || has_brush_option {
            // Pass args through directly to brush
            args.extend(user_args);
        } else {
            // Treat as a command to execute with -c
            args.push("-c".to_string());
            args.push(user_args.join(" "));
        }
    }

    let extra_builtins = |map: &mut std::collections::HashMap<String, crate::brush_core::builtins::Registration>| {
        // Coreutils wrappers
        map.insert("cat".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::CatCommand>());
        map.insert("ls".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::LsCommand>());
        map.insert("cp".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::CpCommand>());
        map.insert("mv".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::MvCommand>());
        map.insert("rm".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::RmCommand>());
        map.insert("mkdir".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::MkdirCommand>());
        map.insert("rmdir".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::RmdirCommand>());
        map.insert("touch".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::TouchCommand>());
        map.insert("head".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::HeadCommand>());
        map.insert("env".to_string(), crate::brush_core::builtins::simple_builtin::<crate::coreutils_builtins::EnvCommand>());

        #[cfg(feature = "wasm-test")]
        {
            map.insert("test".to_string(), crate::brush_core::builtins::simple_builtin::<crate::test_command::TestCommand>());
        }
    };

    crate::brush_shell::embedded::run_custom(args, extra_builtins);
    
    Ok(0)
}
