/// A tiny bridge stub for `uutils` coreutils library in the root crate.
///
/// For now we only log the command name and arguments — the user may later
/// wire this to `coreutils-fork` sub-crate `uu` libraries when they want an in-process call.
pub struct UutilsBridge;

impl UutilsBridge {
    /// Run a command by name; return an exit code.
    pub fn run(name: &str, args: &[String]) -> i32 {
        match name {
            "cat" => {
                if args.len() <= 1 {
                    // Read stdin and write to stdout
                    use std::io::{self, Read, Write};
                    let mut buf = String::new();
                    let mut stdin = io::stdin();
                    let mut stdout = io::stdout();
                    if let Err(_) = stdin.read_to_string(&mut buf) {}
                    let _ = stdout.write_all(buf.as_bytes());
                    0
                } else {
                    // Read each file and write contents
                    use std::fs::File;
                    use std::io::{self, Read, Write};
                    let mut stdout = io::stdout();
                    for filename in &args[1..] {
                        match File::open(filename) {
                            Ok(mut f) => {
                                let mut b = String::new();
                                if let Err(_) = f.read_to_string(&mut b) {}
                                let _ = stdout.write_all(b.as_bytes());
                            }
                            Err(e) => {
                                eprintln!("cat: {}: {}", filename, e);
                                return 1;
                            }
                        }
                    }
                    0
                }
            }
            "pwd" => {
                match std::fs::read_to_string("/dev/host/washm/start_pwd") {
                    Ok(start_pwd) => {
                        let start_pwd = start_pwd.trim();
                        match std::env::current_dir() {
                            Ok(cwd) => {
                                let cwd_path = std::path::Path::new(&cwd);
                                let relative_cwd = cwd_path.strip_prefix("/").unwrap_or(cwd_path);
                                let final_path = std::path::Path::new(start_pwd).join(relative_cwd);
                                println!("{}", final_path.display());
                                0
                            }
                            Err(e) => {
                                eprintln!("pwd: error getting current dir: {}", e);
                                1
                            }
                        }
                    }
                    Err(_) => {
                        match std::env::current_dir() {
                            Ok(p) => {
                                println!("{}", p.display());
                                0
                            }
                            Err(e) => {
                                eprintln!("pwd: {}", e);
                                1
                            }
                        }
                    }
                }
            }
            "ls" => {
                use std::io::Write;
                let mut out = std::io::stdout();
                let targets = if args.len() > 1 { args[1..].to_vec() } else { vec![".".to_string()] };
                for t in targets {
                    match std::fs::read_dir(&t) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let p = entry.file_name();
                                let _ = write!(&mut out, "{}\n", p.to_string_lossy());
                            }
                        }
                        Err(e) => {
                            eprintln!("ls: {}: {}", t, e);
                            return 1;
                        }
                    }
                }
                let _ = out.flush();
                0
            }
            "mkdir" => {
                for dir in &args[1..] {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        eprintln!("mkdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "rmdir" => {
                for dir in &args[1..] {
                    if let Err(e) = std::fs::remove_dir(dir) {
                        eprintln!("rmdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "touch" => {
                use std::io::Write;
                for f in &args[1..] {
                    match std::fs::OpenOptions::new().create(true).append(true).open(f) {
                        Ok(mut file) => { let _ = file.write_all(b""); }
                        Err(e) => { eprintln!("touch: {}: {}", f, e); return 1; }
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
                            eprintln!("rm: {}: {}", f, e2);
                            code = 1;
                        }
                    }
                }
                code
            }
            "cp" => {
                if args.len() < 3 {
                    eprintln!("cp: missing destination file operand");
                    return 1;
                }
                let srcs = &args[1..args.len()-1];
                let dest = &args[args.len()-1];
                // If multiple sources, dest must be a directory
                if srcs.len() > 1 {
                    if let Err(e) = std::fs::create_dir_all(dest) { eprintln!("cp: {}: {}", dest, e); return 1; }
                }
                for s in srcs {
                    let filename = std::path::Path::new(s).file_name().unwrap_or_default();
                    let outp = if srcs.len() > 1 { std::path::Path::new(dest).join(filename) } else { std::path::Path::new(dest).to_path_buf() };
                    if let Err(e) = std::fs::copy(s, outp) { eprintln!("cp: {}: {}", s, e); return 1; }
                }
                0
            }
            "mv" => {
                if args.len() < 3 { eprintln!("mv: missing destination"); return 1; }
                let src = &args[1]; let dest = &args[2];
                if let Err(e) = std::fs::rename(src, dest) { eprintln!("mv: {}: {}", src, e); return 1; }
                0
            }
            "echo" => {
                use std::io::Write;
                let mut out = std::io::stdout();
                if args.len() > 1 {
                    let _ = writeln!(&mut out, "{}", args[1..].join(" "));
                } else {
                    let _ = writeln!(&mut out);
                }
                let _ = out.flush();
                0
            }
            _ => {
                eprintln!("[uutils] command not found: {}", name);
                127
            }
        }
    }
}
