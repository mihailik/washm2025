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
