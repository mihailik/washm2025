/// A tiny dispatcher stub for `uutils` coreutils library in the root crate.
///
/// For now we only log the command name and arguments — the user may later
/// wire this to `coreutils-fork` sub-crate `uu` libraries when they want an in-process call.
pub struct CoreutilsDispatch;

use std::io::Write;

impl CoreutilsDispatch {
    /// Run a command by name; return an exit code.
    pub fn run<R: std::io::Read, W: std::io::Write, E: std::io::Write>(
        name: &str,
        args: &[String],
        mut stdin: R,
        mut stdout: W,
        mut stderr: E,
    ) -> i32 {
        match name {
            "cat" => {
                if args.len() <= 1 {
                    // Read stdin and write to stdout
                    let mut buf = Vec::new();
                    if let Err(_) = std::io::Read::read_to_end(&mut stdin, &mut buf) {}
                    let _ = stdout.write_all(&buf);
                    0
                } else {
                    // Read each file and write contents
                    for filename in &args[1..] {
                        match std::fs::File::open(filename) {
                            Ok(mut f) => {
                                let mut b = Vec::new();
                                if let Err(_) = std::io::Read::read_to_end(&mut f, &mut b) {}
                                let _ = stdout.write_all(&b);
                            }
                            Err(e) => {
                                let _ = writeln!(stderr, "cat: {}: {}", filename, e);
                                return 1;
                            }
                        }
                    }
                    0
                }
            }
            "pwd" => {
                let start_pwd = crate::bridges::host::startup_pwd();
                let start_pwd = start_pwd.trim();
                match std::env::current_dir() {
                    Ok(cwd) => {
                        let cwd_str = cwd.to_string_lossy();
                        let final_path = if cwd_str.starts_with(start_pwd) {
                            std::path::PathBuf::from(cwd_str.as_ref())
                        } else {
                            let cwd_path = std::path::Path::new(&cwd);
                            let relative_cwd = cwd_path.strip_prefix("/").unwrap_or(cwd_path);
                            std::path::Path::new(start_pwd).join(relative_cwd)
                        };
                        let _ = writeln!(stdout, "{}", final_path.display());
                        0
                    }
                    Err(e) => {
                        let _ = writeln!(stderr, "pwd: error getting current dir: {}", e);
                        1
                    }
                }
            }
            "ls" => {
                let mut targets = Vec::new();
                for arg in &args[1..] {
                    if !arg.starts_with('-') {
                        targets.push(arg.clone());
                    }
                }
                if targets.is_empty() {
                    targets.push(".".to_string());
                }
                for t in targets {
                    match std::fs::read_dir(&t) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let p = entry.file_name();
                                let _ = writeln!(stdout, "{}", p.to_string_lossy());
                            }
                        }
                        Err(e) => {
                            let _ = writeln!(stderr, "ls: {}: {}", t, e);
                            return 1;
                        }
                    }
                }
                let _ = stdout.flush();
                0
            }
            "env" => {
                for (key, value) in std::env::vars() {
                    let _ = writeln!(stdout, "{}={}", key, value);
                }
                0
            }
            "head" => {
                use std::io::BufRead;
                let mut n = 10;
                let mut files = Vec::new();
                let mut i = 1;
                while i < args.len() {
                    if args[i] == "-n" && i + 1 < args.len() {
                        if let Ok(val) = args[i+1].parse::<usize>() {
                            n = val;
                            i += 2;
                            continue;
                        }
                    } else if args[i].starts_with("-n") {
                        if let Ok(val) = args[i][2..].parse::<usize>() {
                            n = val;
                            i += 1;
                            continue;
                        }
                    }
                    if !args[i].starts_with('-') {
                        files.push(args[i].clone());
                    }
                    i += 1;
                }

                if files.is_empty() {
                    let reader = std::io::BufReader::new(stdin);
                    for line in reader.lines().take(n) {
                        if let Ok(l) = line {
                            let _ = writeln!(stdout, "{}", l);
                        }
                    }
                } else {
                    for filename in files {
                        match std::fs::File::open(&filename) {
                            Ok(f) => {
                                let reader = std::io::BufReader::new(f);
                                for line in reader.lines().take(n) {
                                    if let Ok(l) = line {
                                        let _ = writeln!(stdout, "{}", l);
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = writeln!(stderr, "head: {}: {}", filename, e);
                                return 1;
                            }
                        }
                    }
                }
                0
            }
            "mkdir" => {
                for dir in &args[1..] {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        let _ = writeln!(stderr, "mkdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "rmdir" => {
                for dir in &args[1..] {
                    if let Err(e) = std::fs::remove_dir(dir) {
                        let _ = writeln!(stderr, "rmdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "touch" => {
                for f in &args[1..] {
                    match std::fs::OpenOptions::new().create(true).append(true).open(f) {
                        Ok(mut file) => { let _ = file.write_all(b""); }
                        Err(e) => { let _ = writeln!(stderr, "touch: {}: {}", f, e); return 1; }
                    }
                }
                0
            }
            "rm" => {
                let mut code = 0;
                for f in &args[1..] {
                    if let Err(e) = std::fs::remove_file(f) {
                        // Try remove_dir_all as a fallback
                        if let Err(e2) = std::fs::remove_dir_all(f) {
                            let _ = writeln!(stderr, "rm: {}: {}", f, e2);
                            code = 1;
                        }
                    }
                }
                code
            }
            "cp" => {
                if args.len() < 3 {
                    let _ = writeln!(stderr, "cp: missing destination file operand");
                    return 1;
                }
                let srcs = &args[1..args.len()-1];
                let dest = &args[args.len()-1];
                // If multiple sources, dest must be a directory
                if srcs.len() > 1 {
                    if let Err(e) = std::fs::create_dir_all(dest) { let _ = writeln!(stderr, "cp: {}: {}", dest, e); return 1; }
                }
                for s in srcs {
                    let filename = std::path::Path::new(s).file_name().unwrap_or_default();
                    let outp = if srcs.len() > 1 { std::path::Path::new(dest).join(filename) } else { std::path::Path::new(dest).to_path_buf() };
                    if let Err(e) = std::fs::copy(s, outp) { let _ = writeln!(stderr, "cp: {}: {}", s, e); return 1; }
                }
                0
            }
            "mv" => {
                if args.len() < 3 { let _ = writeln!(stderr, "mv: missing destination"); return 1; }
                let src = &args[1]; let dest = &args[2];
                if let Err(e) = std::fs::rename(src, dest) { let _ = writeln!(stderr, "mv: {}: {}", src, e); return 1; }
                0
            }
            "echo" => {
                if args.len() > 1 {
                    let _ = writeln!(stdout, "{}", args[1..].join(" "));
                } else {
                    let _ = writeln!(stdout);
                }
                let _ = stdout.flush();
                0
            }
            _ => {
                let _ = writeln!(stderr, "[uutils] command not found: {}", name);
                127
            }
        }
    }
}
