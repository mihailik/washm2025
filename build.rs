use std::env;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use regex::Regex;
use ast_grep_core::{AstGrep, Language};
use ast_grep_language::SupportLang;

fn main() {
    // Note: This build script copies upstream crates to `target/vendor` and patches them.
    // The patched crates are then referenced by `src/lib.rs` using `#[path]` attributes.
    //
    // See [build metad-programming docs](docs/build-meta.md) for maintenance guidance, IDE notes, and
    // troubleshooting steps when upstream sources move or change.
    println!("cargo:rerun-if-changed=brush-fork");
    println!("cargo:rerun-if-changed=coreutils-fork");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/interp_log.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = Path::new(&manifest_dir);
    let vendor_dir = project_root.join("target/vendor");

    // Define mappings: Source Dir -> Dest Dir Name
    let mappings = vec![
        ("brush-fork/brush-core", "brush-core"),
        ("brush-fork/brush-builtins", "brush-builtins"),
        ("brush-fork/brush-parser", "brush-parser"),
        ("brush-fork/brush-interactive", "brush-interactive"),
        ("brush-fork/brush-shell", "brush-shell"),
        ("coreutils-fork/src/uucore", "uucore"),
    ];

    for (src, dest_name) in mappings {
        let src_path = Path::new(src).join("src");
        let dest_path = vendor_dir.join(dest_name).join("src");

        // Validate mapping paths early to give a helpful error if upstream
        // sources are missing or moved.
        if !Path::new(src).exists() {
            panic!("Expected mapped source path '{}' to exist; update `mappings` in build.rs or fetch it.", src);
        }

        // Trigger rebuild if the source subtree changes.
        println!("cargo:rerun-if-changed={}", src);

        if dest_path.exists() {
            fs::remove_dir_all(&dest_path).unwrap();
        }
        fs::create_dir_all(dest_path.parent().unwrap()).unwrap();

        // Copy directory
        let mut options = fs_extra::dir::CopyOptions::new();
        options.overwrite = true;
        options.copy_inside = true;
        
        if let Err(e) = fs_extra::dir::copy(&src_path, dest_path.parent().unwrap(), &options) {
            eprintln!("Failed to copy {:?} to {:?}: {}", src_path, dest_path.parent().unwrap(), e);
            panic!("Failed to copy source");
        }

        // Special handling for uucore embedded_locales.rs
        if dest_name == "uucore" {
             let locales_path = dest_path.join("embedded_locales.rs");
             fs::write(locales_path, r#"
pub fn get_embedded_locale(_key: &str) -> Option<&'static str> {
    None
}
"#).unwrap();
        }

        // For brush-interactive, do NOT copy `src/wasm_interactive.rs` into target/vendor.
       //  The implementation stays in `src/` and remains type-checked and deterministic.
        // Patch files
        patch_files(&dest_path);

        // Create embedded.rs for brush-shell by referencing the canonical
        // `src/embedded_brush_shell.rs` using a relative path computed from
        // the vendored destination. This avoids copying large inline strings
        // into `build.rs` and preserves a single source of truth.
        if dest_name == "brush-shell" {
            let embedded_path = dest_path.join("embedded.rs");
            // Compute a relative path from the vendored `dest_path` to the
            // project's `src/embedded_brush_shell.rs` and write a tiny
            // `include!` that uses that relative path (forward-slash style).
            let project_src = std::env::current_dir().unwrap().join("src/embedded_brush_shell.rs");
            let rel = pathdiff::diff_paths(&project_src, &dest_path).unwrap_or_else(|| Path::new("../../src/embedded_brush_shell.rs").to_path_buf());
            let mut rel_str = rel.to_string_lossy().into_owned();
            rel_str = rel_str.replace("\\", "/");
            let contents = format!("include!(\"{}\");\n", rel_str);
            fs::write(embedded_path, contents).unwrap();
        }

        if dest_name == "brush-core" {
            // Do NOT copy `src/interp_log.rs` into the vendored tree. Instead,
            // `patch_files` will rewrite interpreter error handling sites to call
            // a host shim under `crate::shims::interp_log` so the real
            // implementation lives in `src/` and is type-checked and deterministic.
        }
    }

    verify_no_stubs(&vendor_dir);
}

fn regex_replace(content: String, pattern: &str, replacement: &str, path: &Path) -> String {
    let re = Regex::new(pattern).unwrap();
    if !re.is_match(&content) {
        // Pattern not present - skip patch rather than failing so the build
        // script is idempotent when re-run or when files have already been
        // updated by a prior invocation.
        println!("cargo:warning=Patch pattern not found (skipping): {} in {:?}", pattern, path);
        return content;
    }
    re.replace_all(&content, replacement).to_string()
}

fn ast_grep_replace(content: &str, pattern_str: &str, replacement_str: &str, path: &Path) -> String {
    let lang = SupportLang::Rust;
    let mut grep = AstGrep::new(content, lang);
    let edits = grep.root().replace_all(pattern_str, replacement_str);

    if edits.is_empty() {
        println!("cargo:warning=ast-grep pattern not found (skipping): {} in {:?}", pattern_str, path);
        return content.to_string();
    }

    for edit in edits.into_iter().rev() {
        grep.edit(edit).unwrap();
    }
    grep.generate()
}

// Batched version that applies multiple patterns in a single AST parse
fn ast_grep_replace_batch(content: &str, patterns: &[(&str, &str)], path: &Path) -> String {
    if patterns.is_empty() {
        return content.to_string();
    }

    let lang = SupportLang::Rust;
    let mut grep = AstGrep::new(content, lang);
    let mut any_matches = false;

    // Apply patterns sequentially - each pattern's changes affect the next
    for (pattern_str, replacement_str) in patterns {
        let edits = grep.root().replace_all(pattern_str, replacement_str);
        if !edits.is_empty() {
            any_matches = true;
            // Apply edits in reverse order to maintain correct positions
            for edit in edits.into_iter().rev() {
                grep.edit(edit).unwrap();
            }
        }
    }

    if !any_matches {
        return content.to_string();
    }
    
    grep.generate()
}

fn patch_files(dir: &Path) {
    let crates = vec![
        "brush_core",
        "brush_builtins",
        "brush_parser",
        "brush_interactive",
        "brush_shell",
        "uucore",
    ];

    // Regex for `use CRATE`
    // Matches `use ` followed by CRATE followed by word boundary
    let use_pattern = format!(r"use\s+({})\b", crates.join("|"));
    let use_re = Regex::new(&use_pattern).unwrap();

    // Regex for `CRATE::`
    // Matches (start or non-colon) followed by CRATE followed by ::
    let path_pattern = format!(r"([^:]|^)\b({})::", crates.join("|"));
    let path_re = Regex::new(&path_pattern).unwrap();

    // Regex for inner doc comments `//!`
    let doc_re = Regex::new(r"(?m)^//!").unwrap();

    // Regex for inner attributes `#![...]`
    let inner_attr_re = Regex::new(r"(?m)^#\!\[.*\]").unwrap();

    for entry in WalkDir::new(dir) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs") {
            let path = entry.path();
            let content = fs::read_to_string(path).unwrap();
            let mut new_content = content.clone();

            // Patch uucore/src/lib/mods/locale.rs to include embedded_locales.rs from relative path
            if path.ends_with(std::path::Path::new("uucore/src/lib/mods/locale.rs")) {
                new_content = new_content.replace(
                    r#"include!(concat!(env!("OUT_DIR"), "/embedded_locales.rs"));"#,
                    r#"include!("../../embedded_locales.rs");"#
                );
            }

            // Patch brush-core/src/sys/stubs/users.rs to avoid "operation not supported" on WASI
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/users.rs")) {
                // Use helpers that read startup JSON at /dev/host/washm/startup and cache values
                new_content = ast_grep_replace_batch(&new_content, &[
                    (r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current uid").into())"#, "Ok(crate::stubs_impl::startup_uid())"),
                    (r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current gid").into())"#, "Ok(crate::stubs_impl::startup_gid())"),
                    (r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting effective uid").into())"#, "Ok(crate::stubs_impl::startup_uid())"),
                    (r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting effective gid").into())"#, "Ok(crate::stubs_impl::startup_gid())"),
                    (r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current username").into())"#, "Ok(crate::stubs_impl::startup_username())"),
                ], path);
            }

            // Patch brush-core/src/openfiles.rs to avoid panics when cloning isn't supported on WASI
            if path.ends_with(std::path::Path::new("brush-core/src/openfiles.rs")) {
                // Match `fn clone` and replace the `unwrap()` call with our safe implementation.
                let pattern = r"fn clone(&self) -> Self {
        self.try_clone().unwrap()
    }";
                let replacement = r"fn clone(&self) -> Self {
        crate::stubs_impl::clone_open_file(self)
    }";
                new_content = ast_grep_replace(&new_content, pattern, replacement, path);
            }

            // Patch brush-core/src/sys/stubs/network.rs to use host-provided hostname
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/network.rs")) {
                let pattern = r#"pub(crate) fn get_hostname() -> std::io::Result<std::ffi::OsString> {
    Ok("".into())
}"#;
                let replacement = "pub(crate) fn get_hostname() -> std::io::Result<std::ffi::OsString> {\n    // Use host-provided hostname (via startup JSON) when available\n    Ok(std::ffi::OsString::from(crate::stubs_impl::startup_hostname()))\n}";
                new_content = ast_grep_replace(&new_content, pattern, replacement, path);
            }

            // Re-export startup helpers at `crate::brush_core::sys::stubs` so
            // callers like `crate::brush_core::sys::stubs::startup_username()`
            // resolve correctly from other modules/crates.
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs.rs")) {
                if !new_content.contains("pub use crate::stubs_impl::") {
                    new_content.push_str("\n// Re-export startup helpers from stubs_impl for external use\n");
                    new_content.push_str("pub use crate::stubs_impl::{startup_username, startup_uid, startup_gid};\n");
                }
            }

            // Patch brush-core/src/shell.rs to sync process CWD
            if path.ends_with(std::path::Path::new("brush-core/src/shell.rs")) {
                let pattern = "let oldpwd = std::mem::replace(self.working_dir_mut(), cleaned_path);";
                let replacement = "let oldpwd = std::mem::replace(self.working_dir_mut(), cleaned_path);\n        let _ = std::env::set_current_dir(&self.working_dir);";
                new_content = new_content.replace(pattern, replacement);
            }

            // After all individual patches, verify that no vendored files still
            // contain `NotSupportedOnThisPlatform(` which would indicate we missed
            // a stub. Fail fast so we can fix the mapping rather than shipping
            // a runtime with unexpected generic errors.
            //
            // NOTE: We used to check this here when visiting stubs.rs, but that
            // is flaky because WalkDir order is undefined - if stubs.rs is visited
            // before the individual stub files (users.rs etc), they won't be patched
            // yet and this check will fail. We rely on verify_no_stubs() at the end
            // of main() instead.

            // Patch brush-core/src/sys/stubs/fs.rs to trampoline to our safe implementation
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/fs.rs")) {
                let pattern = r#"pub fn open_null_file() -> Result<std::fs::File, error::Error> {
    Err(error::ErrorKind::NotSupportedOnThisPlatform("opening null file").into())
}"#;
                let to = r#"pub fn open_null_file() -> Result<std::fs::File, error::Error> {
    crate::stubs_impl::open_null_file()
}"#;
                new_content = ast_grep_replace(&new_content, pattern, to, path);
            }

            // Patch brush-core/src/sys/stubs/process.rs to use VFS bridge
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/process.rs")) {
                new_content = ast_grep_replace_batch(&new_content, &[
                    // Replace the spawn function
                    (r#"pub(crate) fn spawn(mut command: std::process::Command) -> std::io::Result<Child> {
    let child = command.spawn()?;
    Ok(Child { inner: child })
}"#, r#"pub(crate) fn spawn(command: std::process::Command) -> std::io::Result<Child> {
    let guest_child = crate::stubs_impl::spawn_process(command)?;
    Ok(Child { inner: guest_child })
}"#),
                    // Replace Child struct to use GuestChild
                    (r#"pub struct Child {
    inner: std::process::Child,
}"#, r#"pub struct Child {
    inner: crate::stubs_impl::GuestChild,
}"#),
                    // Update id method to delegate to GuestChild
                    (r#"pub fn id(&self) -> Option<u32> {
        None
    }"#, r#"pub fn id(&self) -> Option<u32> {
        self.inner.id()
    }"#),
                    // Update wait method to delegate to GuestChild
                    (r#"pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.inner.wait()
    }"#, r#"pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.inner.wait().await
    }"#),
                    // Update wait_with_output method
                    (r#"pub async fn wait_with_output(self) -> std::io::Result<Output> {
        self.inner.wait_with_output()
    }"#, r#"pub async fn wait_with_output(self) -> std::io::Result<Output> {
        self.inner.wait_with_output().await
    }"#),
                ], path);
            }

            // Patch brush-core/src/sys/stubs/signal.rs to trampoline to our safe implementation
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/signal.rs")) {
                new_content = ast_grep_replace_batch(&new_content, &[
                    (r#"pub(crate) fn continue_process(_pid: sys::process::ProcessId) -> Result<(), error::Error> {
    Err(error::ErrorKind::NotSupportedOnThisPlatform("continuing process").into())
}"#, r#"pub(crate) fn continue_process(_pid: sys::process::ProcessId) -> Result<(), error::Error> {
    crate::stubs_impl::continue_process(_pid)
}"#),
                    (r#"pub fn kill_process(_pid: sys::process::ProcessId, _signal: traps::TrapSignal) -> Result<(), error::Error> {
    Err(error::ErrorKind::NotSupportedOnThisPlatform("killing process").into())
}"#, r#"pub fn kill_process(_pid: sys::process::ProcessId, _signal: traps::TrapSignal) -> Result<(), error::Error> {
    crate::stubs_impl::kill_process(_pid, _signal)
}"#),
                ], path);
            }

            // Patch brush-core/src/sys/stubs/commands.rs to trampoline to our safe implementation
            if path.ends_with(std::path::Path::new("brush-core/src/sys/stubs/commands.rs")) {
                let pattern = r#"fn inject_fds(
        &mut self,
        mut open_files: impl Iterator<Item = (ShellFd, openfiles::OpenFile)>,
    ) -> Result<(), error::Error> {
        if open_files.next().is_some() {
            return Err(error::ErrorKind::NotSupportedOnThisPlatform("fd redirections").into());
        }
        Ok(())
    }"#;
                let to = r#"fn inject_fds(
        &mut self,
        mut open_files: impl Iterator<Item = (ShellFd, openfiles::OpenFile)>,
    ) -> Result<(), error::Error> {
        crate::stubs_impl::inject_fds(open_files.collect::<Vec<_>>())
    }"#;
                new_content = ast_grep_replace(&new_content, pattern, to, path);
            }

            // Patch brush-core/src/interp.rs to log interpreter errors
            if path.ends_with(std::path::Path::new("brush-core/src/interp.rs")) {
                let pattern = r#"Err(err) => {
                    // Display the error and convert to an execution result.
                    let _ = shell.display_error(&mut params.stderr(shell), &err).await;
                    result = err.into_result(shell);
                }"#;
                let to = r#"Err(err) => {
                    // Display the error and convert to an execution result.
                    crate::shims::interp_log::log_interp_error(command, &err, shell);
                    let _ = shell.display_error(&mut params.stderr(shell), &err).await;
                    result = err.into_result(shell);
                }"#;
                new_content = ast_grep_replace(&new_content, pattern, to, path);
            }


            // Specific file patches (run early to avoid conflicts with general patches)
            if path.ends_with(std::path::Path::new("brush-builtins/src/lib.rs")) {
                // The macro must be defined BEFORE the modules that use it are declared
                // Move the macro definition to the top of the file
                if new_content.contains("macro_rules! minus_or_plus_flag_arg") {
                    // Remove #[macro_export] since we want it module-local
                    new_content = new_content.replace("#[macro_export]\nmacro_rules! minus_or_plus_flag_arg", "macro_rules! minus_or_plus_flag_arg");
                    
                    // Extract the macro definition
                    let macro_start = new_content.find("/// Macro to define a struct").unwrap();
                    let macro_end = new_content[macro_start..].find("\n}\n").unwrap() + macro_start + 3;
                    let macro_def = new_content[macro_start..macro_end].to_string();
                    
                    // Remove the macro from its current location
                    new_content.replace_range(macro_start..macro_end, "");
                    
                    // Insert it at the top of the file (after the doc comment if any)
                    let insert_pos = if new_content.starts_with("//") {
                        new_content.find("\n\n").unwrap_or(0) + 2
                    } else {
                        0
                    };
                    
                    new_content.insert_str(insert_pos, &format!("{}\n\n", macro_def));
                }
            }

            if path.ends_with(std::path::Path::new("brush-interactive/src/lib.rs")) {
                // Disable original modules/exports to avoid conflicts
                // Comment out cfg attributes for backends
                let cfg_re = Regex::new(r#"(?m)^#\[cfg\(feature = "(reedline|basic|minimal)"\)\]"#).unwrap();
                new_content = cfg_re.replace_all(&new_content, "// $0").to_string();

                // Comment out mod declarations
                let mod_re = Regex::new(r"(?m)^(pub\s+)?mod\s+(reedline|basic|minimal)\s*;").unwrap();
                new_content = mod_re.replace_all(&new_content, "// $0").to_string();

                // Comment out pub use declarations
                let use_re = Regex::new(r"(?m)^pub\s+use\s+(reedline|basic|minimal)::.*").unwrap();
                new_content = use_re.replace_all(&new_content, "// $0").to_string();

                new_content.push_str("\n/// Use project-implemented `wasm_interactive` via include! so the real code stays in `src/`\n");
                new_content.push_str("pub mod wasm_interactive { include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/wasm_interactive.rs\")); }\n");
                new_content.push_str("\npub use wasm_interactive::{WasmInputBackend, ReedlineInputBackend, BasicInputBackend, MinimalInputBackend};");
                
                // Inline refs module to avoid resolution issues
                if new_content.contains("mod refs;") {
                    println!("cargo:warning=Inlining refs module");
                    new_content = ast_grep_replace(&new_content, "mod refs;", r#"pub type ShellRef = std::sync::Arc<tokio::sync::Mutex<super::brush_core::Shell>>;"#, path);
                } else {
                    println!("cargo:warning=Could not find 'mod refs;' to inline");
                }
                
                // Remove pub use refs::ShellRef; since we defined it directly
                // Comment it out rather than removing to preserve file structure
                let use_refs_re = Regex::new(r"(?m)^pub\s+use\s+refs::ShellRef\s*;").unwrap();
                new_content = use_refs_re.replace_all(&new_content, "// $0").to_string();
            }

            // 0. Replace `crate::` with `crate::CURRENT_CRATE::`
            // Identify which "crate" this file belongs to. Normalize path separators
            // so checks work consistently on Windows and Unix.
            let path_str = path.to_string_lossy();
            let path_norm = path_str.replace("\\", "/");
            let current_crate = if path_norm.contains("/brush-parser/") { Some("brush_parser") }
                else if path_norm.contains("/brush-core/") { Some("brush_core") }
                else if path_norm.contains("/brush-builtins/") { Some("brush_builtins") }
                else if path_norm.contains("/brush-interactive/") { Some("brush_interactive") }
                else if path_norm.contains("/brush-shell/") { Some("brush_shell") }
                else if path_norm.contains("/uucore/") { Some("uucore") }
                else { None };

            if let Some(c_name) = current_crate {
                // Replace `crate::` with `crate::c_name::`
                // But avoid `pub(crate)` which doesn't have `::`
                // Regex: `\bcrate::`
                // Only replace bare `crate::` occurrences that are NOT followed by a
                // top-level vendored crate name. If `crate::brush_parser::` is present we
                // want to keep it as-is (it's a cross-crate reference). We therefore use
                // a negative lookahead to avoid rewriting those cases into
                // `crate::brush_core::brush_parser::...` which would be invalid.
                // We can't use look-around with the `regex` crate, so match the
                // `crate::` plus the immediate following identifier and decide
                // whether to rewrite on a per-match basis.
                let crate_re = Regex::new(r"\bcrate::([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
                let crate_names_vec = crates.clone();
                new_content = crate_re.replace_all(&new_content, |caps: &regex::Captures| {
                    let next = caps.get(1).unwrap().as_str();
                    // If the next segment is a vendored crate name, leave alone
                    // (this is a cross-crate reference like `crate::brush_parser`).
                    if crate_names_vec.contains(&next) {
                        caps[0].to_string()
                    } else {
                        format!("crate::{}::{}", c_name, next)
                    }
                }).to_string();
                // Revert any accidental rewrites of our host trampoline module
                // so trampolines injected earlier continue to call the real
                // `crate::stubs_impl` in the host crate rather than
                // `crate::<current_crate>::stubs_impl` which does not exist.
                let host_stub_prefix = format!("crate::{}::stubs_impl", c_name);
                if new_content.contains(&host_stub_prefix) {
                    new_content = new_content.replace(&host_stub_prefix, "crate::stubs_impl");
                }
                // Revert accidental rewrites of the interp_log_shim trampoline which should
                // call into the host `crate::shims::interp_log` rather than
                // `crate::<vendored>::shims::interp_log` after the generic `crate::`
                // qualification rewrites above.
                let interp_shim_prefix = format!("crate::{}::shims::interp_log", c_name);
                if new_content.contains(&interp_shim_prefix) {
                    new_content = new_content.replace(&interp_shim_prefix, "crate::shims::interp_log");
                }

                // Handle `use crate::{...}` style imports which our simple
                // `crate::X` matcher doesn't catch. Rewrite them to include
                // the current crate qualification: `crate::{...}` ->
                // `crate::c_name::{...}` so internal imports resolve.
                if new_content.contains("crate::{") {
                    new_content = new_content.replace("crate::{", &format!("crate::{}::{{", c_name));
                }

                
                // Fix macro invocations - macros exported with #[macro_export] 
                // are available at the crate root and should be called without any path prefix
                // So crate::brush_builtins::macro! should be just macro!
                // Since we've made the macro module-local (removed #[macro_export]), 
                // we don't need (and can't use) import statements
                let macro_re = Regex::new(&format!(r"crate::{}::([a-zA-Z_][a-zA-Z0-9_]*)!", c_name)).unwrap();
                new_content = macro_re.replace_all(&new_content, "$1!").to_string();
            }

            // 1. Replace `//!` with `//`
            new_content = doc_re.replace_all(&new_content, "//").to_string();

            // 1.5 Replace `#![...]` with `// #![...]`
            new_content = inner_attr_re.replace_all(&new_content, "// $0").to_string();

            // 2. Replace `use CRATE` with `use crate::CRATE`
            new_content = use_re.replace_all(&new_content, |caps: &regex::Captures| {
                format!("use crate::{}", &caps[1])
            }).to_string();

            // 3. Replace `CRATE::` with `crate::CRATE::` (if not preceded by :)
            new_content = path_re.replace_all(&new_content, |caps: &regex::Captures| {
                format!("{}crate::{}::", &caps[1], &caps[2])
            }).to_string();

            // Patch: Redirect crossterm to our local shim
            new_content = new_content.replace("crossterm::", "crate::shims::crossterm::");
            new_content = new_content.replace("use crossterm;", "use crate::shims::crossterm as crossterm;");
            new_content = new_content.replace("use crossterm ", "use crate::shims::crossterm ");

            // 4. Special case: Disable conflicting blanket impl in brush-core/src/error.rs
            if path.ends_with(std::path::Path::new("brush-core/src/error.rs")) {
                new_content = new_content.replace(
                    "impl<T> From<T> for Error",
                    &format!("include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/brush_error_impls.rs\"));\n#[cfg(any())] impl<T> From<T> for Error")
                );
            }

            // Patch: Expose brushctl for use in stubs_impl
            if path.ends_with(std::path::Path::new("brush-shell/src/lib.rs")) {
                 new_content = ast_grep_replace(&new_content, "mod brushctl;", "pub mod brushctl;", path);
            }
            if path.ends_with(std::path::Path::new("brush-shell/src/brushctl.rs")) {
                 new_content = ast_grep_replace(&new_content, "pub(crate) trait ShellBuilderBrushBuiltinExt", "pub trait ShellBuilderBrushBuiltinExt", path);
            }

            // 5. Patch `mod name;` with absolute paths
            // Regex: ^\s*(pub(?:\([^)]+\))?\s+)?mod\s+(\w+)\s*;
            let mod_re = Regex::new(r"(?m)^\s*((?:pub(?:\([^)]+\))?\s+)*)mod\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;").unwrap();
            
            new_content = mod_re.replace_all(&new_content, |caps: &regex::Captures| {
                let vis = &caps[1];
                let mod_name = &caps[2];
                
                // Determine potential paths
                let parent_dir = path.parent().unwrap();
                let file_stem = path.file_stem().unwrap().to_str().unwrap();
                
                let mut search_dirs = vec![];
                if file_stem == "lib" || file_stem == "mod" {
                    search_dirs.push(parent_dir.to_path_buf());
                } else {
                    search_dirs.push(parent_dir.join(file_stem));
                }
                
                let mut found_path = None;
                for dir in search_dirs {
                    let p1 = dir.join(format!("{}.rs", mod_name));
                    let p2 = dir.join(mod_name).join("mod.rs");
                    
                    if p1.exists() {
                        found_path = Some(p1);
                        break;
                    }
                    if p2.exists() {
                        found_path = Some(p2);
                        break;
                    }
                }
                
                if let Some(p) = found_path {
                    if mod_name == "refs" {
                        println!("cargo:warning=Resolved refs to: {:?}", p);
                    }
                    // Use a raw string literal for the path attribute so backslashes
                    // on Windows don't produce unknown escape errors when compiled.
                    format!("#[path = r\"{}\"]\n{}mod {};", p.display(), vis, mod_name)
                } else {
                    // If not found, leave it as is (might be a macro-generated module or cfg-ed out)
                    println!("cargo:warning=Module not found: {} in {:?}", mod_name, parent_dir);
                    caps[0].to_string()
                }
            }).to_string();

            // 6. Specific file patches
            if path.ends_with(std::path::Path::new("brush-core/src/expansion.rs")) {
                 new_content = format!("use std::future::Future;\n{}", new_content);
            }

            if path.ends_with(std::path::Path::new("brush-shell/src/entry.rs")) {
                 new_content = ast_grep_replace_batch(&new_content, &[
                     ("async fn run_async", "pub async fn run_async"),
                     ("fn try_parse_from", "pub fn try_parse_from"),
                 ], path);
                 
                 // Add extra builtins support (after crate:: patching has already happened)
                 if new_content.contains("crate::brush_core::Shell::builder()") {
                      // Replace the builtin registration to add extra builtins
                      new_content = new_content.replace(
                          "    // Add builtins.\n    let shell = shell.default_builtins(builtin_set).brush_builtins();",
                          "    // Add builtins.\n    let shell = crate::stubs_impl::configure_shell(shell, builtin_set);"
                      );
                  }
                 
                 // Patch MinimalInputBackend instantiation
                 new_content = ast_grep_replace_batch(&new_content, &[
                     ("let mut ui = brush_interactive::MinimalInputBackend;", "let mut ui = brush_interactive::MinimalInputBackend::new();"),
                     ("let mut ui = crate::brush_interactive::MinimalInputBackend;", "let mut ui = crate::brush_interactive::MinimalInputBackend::new();"),
                 ], path);
            }
            
            if path.ends_with(std::path::Path::new("brush-shell/src/lib.rs")) {
                 new_content.push_str("\npub mod embedded;");
            }

            // Patch InputBackend trait and usages
            if new_content.contains("trait InputBackend") {
                 new_content = new_content.replace("fn read_line", "async fn read_line");
            }
            
            // Patch impls and calls
            if (new_content.contains("impl InputBackend") || path.to_string_lossy().contains("interactive")) && !path.ends_with(std::path::Path::new("wasm_interactive.rs")) {
                 // Make impl methods async
                 // Use regex to avoid double async
                 let async_re = Regex::new(r"(?m)(async\s+)?fn\s+read_line").unwrap();
                 new_content = async_re.replace_all(&new_content, "async fn read_line").to_string();
                 
                 // Patch call sites: .read_line(prompt)? -> .read_line(prompt).await?
                 let call_re = Regex::new(r"\.read_line\(([^)]*)\)\?").unwrap();
                 new_content = call_re.replace_all(&new_content, ".read_line($1).await?").to_string();
            }

            // Deduplicate identical `use ...;` imports which can be
            // introduced by earlier transformations or upstream changes.
            // Re-importing the same item can cause `E0252` (defined
            // multiple times) so collapse exact duplicate `use` lines.
            {
                use std::collections::HashSet;
                let mut seen: HashSet<String> = HashSet::new();
                let mut out_lines: Vec<&str> = Vec::new();
                for line in new_content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("use ") && trimmed.ends_with(";") {
                        if seen.contains(trimmed) {
                            // skip duplicate import
                            continue;
                        }
                        seen.insert(trimmed.to_string());
                    }
                    out_lines.push(line);
                }
                new_content = out_lines.join("\n");
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
            }

            if content != new_content {
                fs::write(path, new_content).unwrap();
            }
        }
    }

    // After patching all files in this directory, perform a targeted check for stubs
    // to ensure we didn't miss anything in the stubs directory.
    // This is safe here because the loop above has finished processing all files.
    let stubs_dir = dir.join("sys/stubs");
    if stubs_dir.exists() {
        let mut found = vec![];
        for entry in WalkDir::new(&stubs_dir) {
            let entry = entry.unwrap();
            if entry.file_type().is_file() {
                let p = entry.path();
                if let Ok(s) = fs::read_to_string(p) {
                    if s.contains("NotSupportedOnThisPlatform(") {
                        found.push(p.to_path_buf());
                    }
                }
            }
        }
        if !found.is_empty() {
            panic!("Unpatched NotSupportedOnThisPlatform stubs found in {:?}: {:?}", stubs_dir, found);
        }
    }
}

fn verify_no_stubs(out_dir: &Path) {
    println!("Verifying no unpatched stubs in {:?}", out_dir);
    let mut found = vec![];
    for entry in WalkDir::new(out_dir) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs") {
            let path = entry.path();
            // Skip windows/unix specific modules that are not used on WASI
            // (Adjust this logic if we target Unix/Windows)
            if path.to_string_lossy().contains("/sys/windows/") || path.to_string_lossy().contains("/sys/unix/") {
                continue;
            }
            // Skip error definition
            if path.ends_with("brush-core/src/error.rs") {
                continue;
            }

            let content = fs::read_to_string(path).unwrap();
            if content.contains("NotSupportedOnThisPlatform") {
                found.push(path.to_path_buf());
            }
        }
    }
    
    if !found.is_empty() {
        panic!("BUILD FAILED: Unpatched NotSupportedOnThisPlatform stubs found in:\n{:#?}", found);
    }
}
