use ion_shell::{BuiltinMap, Value, Shell};
use ion_shell::builtins::Status;
use std::io::{stdin, BufReader};
use std::sync::Arc;

pub fn run(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    // Minimal bindings that match `ion`'s main.rs startup.
    let mut builtins = BuiltinMap::default();
    builtins
        .with_unsafe()
        .add("debug", &ion_shell::builtins::builtin_debug, "Toggle debug mode")
        .add("exec", &ion_shell::builtins::builtin_exec, "Replace the shell with the given command.")
        .add("exit", &ion_shell::builtins::builtin_exit, "Exits the current session")
        .add("suspend", &ion_shell::builtins::builtin_suspend, "Suspends the shell with a SIGTSTOP signal");

    let stdin_is_tty = atty::is(atty::Stream::Stdin);
    let mut shell = Shell::with_builtins(builtins);

    // Mirror a few of Ion's built-in behaviors.
    if stdin_is_tty {
        // Ion sets process group and ttys; we leave it as a best-effort no-op here
        // until a proper ConPTY / CreateProcess backend is implemented.
    }

    shell.set_background_event(Some(Arc::new(|njob, pid, kind| match kind {
        ion_shell::BackgroundEvent::Added => eprintln!("ion: bg [{}] {}", njob, pid),
        ion_shell::BackgroundEvent::Stopped => eprintln!("ion: ([{}] {}) Stopped", njob, pid),
        ion_shell::BackgroundEvent::Resumed => eprintln!("ion: ([{}] {}) Running", njob, pid),
        ion_shell::BackgroundEvent::Exited(status) => eprintln!("ion: ([{}] {}) exited with {}", njob, pid, status),
        ion_shell::BackgroundEvent::Errored(error) => eprintln!("ion: ([{}] {}) errored: {}", njob, pid, error),
    })));

    // Equivalent to setting args
    if !args.is_empty() {
        let argv: Vec<ion_shell::Value> = args.iter().cloned().map(Value::Str).collect();
        shell.variables_mut().set("args", Value::Array(argv));
    }

    // For now, only support non-interactive / script usage in-process
    // Interactive mode is heavier; we prefer using the ion binary until we
    // implement ConPTY and `InteractiveShell` export.
    if stdin_is_tty {
        // keep it simple: fallback to script, non-interactive for now
    }

    let err = shell.execute_command(BufReader::new(stdin())).and_then(|_| shell.wait_for_background().map_err(Into::into));

    if let Err(why) = err {
        eprintln!("ion: {}", why);
        return Ok(1);
    }
    Ok(shell.previous_status().as_os_code())
}
