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
        cwd: &std::path::Path,
        mut stdin: R,
        mut stdout: W,
        mut stderr: E,
    ) -> i32 {
        let resolve = |p: &str| {
            let path = std::path::Path::new(p);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            // Simple normalization: remove double slashes
            let s = resolved.to_string_lossy().replace("//", "/");
            std::path::PathBuf::from(s)
        };

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
                        match std::fs::File::open(resolve(filename)) {
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
                    match std::fs::read_dir(resolve(&t)) {
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
                        match std::fs::File::open(resolve(&filename)) {
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
                    if let Err(e) = std::fs::create_dir_all(resolve(dir)) {
                        let _ = writeln!(stderr, "mkdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "rmdir" => {
                for dir in &args[1..] {
                    if let Err(e) = std::fs::remove_dir(resolve(dir)) {
                        let _ = writeln!(stderr, "rmdir: {}: {}", dir, e);
                        return 1;
                    }
                }
                0
            }
            "touch" => {
                for f in &args[1..] {
                    match std::fs::OpenOptions::new().create(true).append(true).open(resolve(f)) {
                        Ok(mut file) => { let _ = file.write_all(b""); }
                        Err(e) => { let _ = writeln!(stderr, "touch: {}: {}", f, e); return 1; }
                    }
                }
                0
            }
            "rm" => {
                let mut code = 0;
                for f in &args[1..] {
                    let resolved = resolve(f);
                    if let Err(_e) = std::fs::remove_file(&resolved) {
                        // Try remove_dir_all as a fallback
                        if let Err(e2) = std::fs::remove_dir_all(&resolved) {
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
                let resolved_dest = resolve(dest);
                // If multiple sources, dest must be a directory
                if srcs.len() > 1 {
                    if let Err(e) = std::fs::create_dir_all(&resolved_dest) { let _ = writeln!(stderr, "cp: {}: {}", dest, e); return 1; }
                }
                for s in srcs {
                    let resolved_src = resolve(s);
                    let filename = std::path::Path::new(s).file_name().unwrap_or_default();
                    let outp = if srcs.len() > 1 { resolved_dest.join(filename) } else { resolved_dest.clone() };
                    if let Err(e) = std::fs::copy(resolved_src, outp) { let _ = writeln!(stderr, "cp: {}: {}", s, e); return 1; }
                }
                0
            }
            "mv" => {
                if args.len() < 3 { let _ = writeln!(stderr, "mv: missing destination"); return 1; }
                let src = &args[1]; let dest = &args[2];
                if let Err(e) = std::fs::rename(resolve(src), resolve(dest)) { let _ = writeln!(stderr, "mv: {}: {}", src, e); return 1; }
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

#[cfg(feature = "wasm-test")]
pub fn register_dispatch_tests() {
    crate::test_harness::register_test("test_path_resolution", test_path_resolution);
    crate::test_harness::register_test("test_coreutils_with_cwd", test_coreutils_with_cwd);
}

#[cfg(feature = "wasm-test")]
fn test_path_resolution() {
    let cwd = std::path::Path::new("/C:/Users/mihai");
    
    let resolve = |p: &str| {
        let path = std::path::Path::new(p);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    };

    assert_eq!(resolve("test.txt"), std::path::Path::new("/C:/Users/mihai/test.txt"));
    assert_eq!(resolve("/D:/other"), std::path::Path::new("/D:/other"));
    assert_eq!(resolve("subdir/file.txt"), std::path::Path::new("/C:/Users/mihai/subdir/file.txt"));
    assert_eq!(resolve("./file.txt"), std::path::Path::new("/C:/Users/mihai/./file.txt"));
    assert_eq!(resolve("../other/file.txt"), std::path::Path::new("/C:/Users/mihai/../other/file.txt"));
    assert_eq!(resolve("/"), std::path::Path::new("/"));
    assert_eq!(resolve("/C:/"), std::path::Path::new("/C:/"));
}

#[cfg(feature = "wasm-test")]
fn test_coreutils_with_cwd() {
    use std::io::Cursor;
    
    // Create a temporary directory for testing
    let test_dir = std::env::temp_dir().join("washm_test_cwd");
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).expect("Failed to create test directory");
    
    // Create a subdirectory
    let sub_dir = test_dir.join("subdir");
    std::fs::create_dir_all(&sub_dir).expect("Failed to create subdirectory");
    
    // Create a test file in the main directory
    std::fs::write(test_dir.join("test.txt"), b"Hello from test.txt\n")
        .expect("Failed to write test.txt");
    
    // Create a test file in the subdirectory
    std::fs::write(sub_dir.join("sub.txt"), b"Hello from subdir\n")
        .expect("Failed to write sub.txt");
    
    // Test 1: cat with relative path
    {
        let stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        
        let args = vec!["cat".to_string(), "test.txt".to_string()];
        let exit_code = CoreutilsDispatch::run(
            "cat",
            &args,
            &test_dir,
            stdin,
            &mut stdout,
            &mut stderr,
        );
        
        assert_eq!(exit_code, 0, "cat should succeed");
        assert_eq!(stdout, b"Hello from test.txt\n");
    }
    
    // Test 2: cat with relative path to subdirectory
    {
        let stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        
        let args = vec!["cat".to_string(), "subdir/sub.txt".to_string()];
        let exit_code = CoreutilsDispatch::run(
            "cat",
            &args,
            &test_dir,
            stdin,
            &mut stdout,
            &mut stderr,
        );
        
        assert_eq!(exit_code, 0, "cat should succeed with subdir path");
        assert_eq!(stdout, b"Hello from subdir\n");
    }
    
    // Test 3: ls in current directory
    {
        let stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        
        let args = vec!["ls".to_string()];
        let exit_code = CoreutilsDispatch::run(
            "ls",
            &args,
            &test_dir,
            stdin,
            &mut stdout,
            &mut stderr,
        );
        
        assert_eq!(exit_code, 0, "ls should succeed");
        let output = String::from_utf8_lossy(&stdout);
        assert!(output.contains("test.txt"), "ls output should contain test.txt");
        assert!(output.contains("subdir"), "ls output should contain subdir");
    }
    
    // Test 4: touch creates file relative to cwd
    {
        let stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        
        let args = vec!["touch".to_string(), "newfile.txt".to_string()];
        let exit_code = CoreutilsDispatch::run(
            "touch",
            &args,
            &test_dir,
            stdin,
            &mut stdout,
            &mut stderr,
        );
        
        assert_eq!(exit_code, 0, "touch should succeed");
        assert!(test_dir.join("newfile.txt").exists(), "newfile.txt should exist");
    }
    
    // Test 5: mkdir creates directory relative to cwd
    {
        let stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        
        let args = vec!["mkdir".to_string(), "newdir".to_string()];
        let exit_code = CoreutilsDispatch::run(
            "mkdir",
            &args,
            &test_dir,
            stdin,
            &mut stdout,
            &mut stderr,
        );
        
        assert_eq!(exit_code, 0, "mkdir should succeed");
        assert!(test_dir.join("newdir").is_dir(), "newdir should exist");
    }
    
    // Clean up
    let _ = std::fs::remove_dir_all(&test_dir);
}
