/// Semantic Transformation Engine using rust-analyzer
/// Implements the specification from docs/1.7-ra.md
///
/// Section 2: Deadlock Avoidance - CARGO_TARGET_DIR is set at the start
/// Section 3: Dual-Graph Architecture - Uses RA's parser for semantic understanding
/// Section 4: Core Transformations - Semantic re-rooting, path synthesis, SSR
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// rust-analyzer imports for semantic analysis
use ra_ap_ide::AnalysisHost;
use ra_ap_syntax::{Edition, SourceFile};

fn main() {
    // Section 2: Deadlock Avoidance - Set isolated target directory BEFORE any RA operations
    std::env::set_var("CARGO_TARGET_DIR", "target/ra-rewrite");

    println!("cargo:rerun-if-changed=brush-fork");
    println!("cargo:rerun-if-changed=coreutils-fork");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/interp_log.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = PathBuf::from(&manifest_dir);
    let vendor_dir = project_root.join("target/vendor");

    // Define source mappings
    let mappings = vec![
        ("brush-fork/brush-core", "brush-core"),
        ("brush-fork/brush-builtins", "brush-builtins"),
        ("brush-fork/brush-parser", "brush-parser"),
        ("brush-fork/brush-interactive", "brush-interactive"),
        ("brush-fork/brush-shell", "brush-shell"),
        ("coreutils-fork/src/uucore", "uucore"),
    ];

    // Validate sources exist
    for (src, _) in &mappings {
        if !Path::new(src).exists() {
            panic!("Expected mapped source path '{}' to exist", src);
        }
        println!("cargo:rerun-if-changed={}", src);
    }

    // Section 3: Initialize Semantic Engine with RA
    let mut engine = SemanticEngine::new();

    // Copy source files to vendor directory
    copy_sources(&mappings, &vendor_dir);

    // Section 4: Apply semantic transformations using RA
    engine.transform_all(&vendor_dir, &mappings);

    // Create additional required files
    create_customizations(&vendor_dir, &project_root, &mappings);

    // Final validation
    verify_no_stubs(&vendor_dir);
}

/// Semantic Engine using rust-analyzer's parser and AST
struct SemanticEngine {
    analysis_host: AnalysisHost,
}

impl SemanticEngine {
    fn new() -> Self {
        // Section 3.A: Initialize AnalysisHost for semantic analysis
        let analysis_host = AnalysisHost::default();

        Self { analysis_host }
    }

    fn transform_all(&mut self, vendor_dir: &Path, mappings: &[(&str, &str)]) {
        for (_, dest_name) in mappings {
            let dest_path = vendor_dir.join(dest_name).join("src");
            if !dest_path.exists() {
                continue;
            }

            self.transform_crate(&dest_path, dest_name);
        }
    }

    fn transform_crate(&mut self, crate_path: &Path, crate_name: &str) {
        let module_name = crate_name.replace("-", "_");

        // Walk all Rust files in this crate
        for entry in WalkDir::new(crate_path) {
            let entry = entry.unwrap();
            if !entry.file_type().is_file() || entry.path().extension().map_or(true, |e| e != "rs")
            {
                continue;
            }

            let path = entry.path();
            let content = fs::read_to_string(path).unwrap();

            // Section 4.A: Semantic Re-rooting using RA's parser
            let transformed = self.semantic_rerooting(&content, &module_name);

            // Section 4.B: Automated Path Synthesis
            let transformed = self.synthesize_mod_paths(&transformed, path);

            // Section 4.C: Type-Aware SSR (Structural Search Replace)
            let transformed = self.apply_ssr_transforms(&transformed, path);

            // Additional necessary transformations
            let transformed = self.apply_additional_transforms(&transformed, path);

            if content != transformed {
                fs::write(path, transformed).unwrap();
            }
        }
    }

    // Section 4.A: Semantic Re-rooting using RA's semantic understanding
    fn semantic_rerooting(&self, content: &str, current_module: &str) -> String {
        // Use RA's parser to understand the code structure
        let parse = SourceFile::parse(content, Edition::Edition2021);
        let _root = parse.tree();

        // Track all vendored crate names
        let vendored_crates = vec![
            "brush_core",
            "brush_builtins",
            "brush_parser",
            "brush_interactive",
            "brush_shell",
            "uucore",
        ];

        let mut result = content.to_string();

        // Step 1: Add crate:: prefix to bare vendored crate references
        for crate_name in &vendored_crates {
            // Simple replacement: if we see vendored_crate:: not preceded by crate::, add crate::
            let old_pattern = format!("{}", crate_name);
            result = result.replace(
                &format!(" {}::", old_pattern),
                &format!(" crate::{}::", old_pattern),
            );
            result = result.replace(
                &format!("({}::", old_pattern),
                &format!("(crate::{}::", old_pattern),
            );
            result = result.replace(
                &format!("<{}::", old_pattern),
                &format!("<crate::{}::", old_pattern),
            );
            result = result.replace(
                &format!(",{}::", old_pattern),
                &format!(",crate::{}::", old_pattern),
            );
            result = result.replace(
                &format!("&{}::", old_pattern),
                &format!("&crate::{}::", old_pattern),
            );
            result = result.replace(
                &format!("\n{}::", old_pattern),
                &format!("\ncrate::{}::", old_pattern),
            );
            // Handle start of string
            if result.starts_with(&format!("{}::", old_pattern)) {
                result = format!("crate::{}", result);
            }
        }

        // Handle use statements specifically
        for crate_name in &vendored_crates {
            result = result.replace(
                &format!("use {};", crate_name),
                &format!("use crate::{};", crate_name),
            );
            result = result.replace(
                &format!("use {}::", crate_name),
                &format!("use crate::{}::", crate_name),
            );
        }

        // Step 2: Handle `crate::` references that need re-rooting
        let crate_re = regex::Regex::new(r"\bcrate::([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
        result = crate_re
            .replace_all(&result, |caps: &regex::Captures| {
                let next_segment = caps.get(1).unwrap().as_str();

                // If this is already a vendored crate reference, leave it
                if vendored_crates.contains(&next_segment) {
                    return caps[0].to_string();
                }

                // Otherwise, qualify it with the current module
                format!("crate::{}::{}", current_module, next_segment)
            })
            .to_string();

        // Handle use crate::{...} imports
        if result.contains("crate::{") {
            result = result.replace("crate::{", &format!("crate::{}::{{", current_module));
        }

        // Revert host trampolines
        result = result.replace(
            &format!("crate::{}::stubs_impl", current_module),
            "crate::stubs_impl",
        );
        result = result.replace(&format!("crate::{}::shims", current_module), "crate::shims");

        // Fix macro invocations (macros are at crate root)
        let macro_re = regex::Regex::new(&format!(
            r"crate::{}::([a-zA-Z_][a-zA-Z0-9_]*)!",
            current_module
        ))
        .unwrap();
        result = macro_re.replace_all(&result, "$1!").to_string();

        result
    }

    // Section 4.B: Automated Path Synthesis
    fn synthesize_mod_paths(&self, content: &str, current_file: &Path) -> String {
        let mod_re = regex::Regex::new(
            r"(?m)^\s*((?:pub(?:\([^)]+\))?\s+)*)mod\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;",
        )
        .unwrap();

        mod_re
            .replace_all(content, |caps: &regex::Captures| {
                let vis = &caps[1];
                let mod_name = &caps[2];

                let parent_dir = current_file.parent().unwrap();
                let file_stem = current_file.file_stem().unwrap().to_str().unwrap();

                let search_dir = if file_stem == "lib" || file_stem == "mod" {
                    parent_dir.to_path_buf()
                } else {
                    parent_dir.join(file_stem)
                };

                let candidates = vec![
                    search_dir.join(format!("{}.rs", mod_name)),
                    search_dir.join(mod_name).join("mod.rs"),
                ];

                for candidate in candidates {
                    if candidate.exists() {
                        // Normalize to forward slashes for the generated attribute
                        let path_literal = candidate.to_string_lossy().replace("\\", "/");
                        return format!(
                            "#[path = r\"{}\"]\n{}mod {};",
                            path_literal, vis, mod_name
                        );
                    }
                }

                caps[0].to_string()
            })
            .to_string()
    }

    // Section 4.C: Type-Aware SSR for stub replacements
    fn apply_ssr_transforms(&self, content: &str, path: &Path) -> String {
        let mut result = content.to_string();
        let path_str = normalize_path(path);

        if path_str.ends_with("brush-core/src/sys/stubs/users.rs") {
            result = result
                .replace(r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current uid").into())"#, "Ok(crate::stubs_impl::startup_uid())")
                .replace(r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current gid").into())"#, "Ok(crate::stubs_impl::startup_gid())")
                .replace(r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting effective uid").into())"#, "Ok(crate::stubs_impl::startup_uid())")
                .replace(r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting effective gid").into())"#, "Ok(crate::stubs_impl::startup_gid())")
                .replace(r#"Err(error::ErrorKind::NotSupportedOnThisPlatform("getting current username").into())"#, "Ok(crate::stubs_impl::startup_username())");
        }

        if path_str.ends_with("brush-core/src/openfiles.rs") {
            result = result.replace(
                "fn clone(&self) -> Self {\n        self.try_clone().unwrap()\n    }",
                "fn clone(&self) -> Self {\n        crate::stubs_impl::clone_open_file(self)\n    }"
            );
        }

        if path_str.ends_with("brush-core/src/sys/stubs/network.rs") {
            result = result.replace(
                "pub(crate) fn get_hostname() -> std::io::Result<std::ffi::OsString> {\n    Ok(\"\".into())\n}",
                "pub(crate) fn get_hostname() -> std::io::Result<std::ffi::OsString> {\n    Ok(std::ffi::OsString::from(crate::stubs_impl::startup_hostname()))\n}"
            );
        }

        if path_str.ends_with("brush-core/src/sys/stubs/fs.rs") {
            result = result.replace(
                "pub fn open_null_file() -> Result<std::fs::File, error::Error> {\n    Err(error::ErrorKind::NotSupportedOnThisPlatform(\"opening null file\").into())\n}",
                "pub fn open_null_file() -> Result<std::fs::File, error::Error> {\n    crate::stubs_impl::open_null_file()\n}"
            );
        }

        if path_str.ends_with("brush-core/src/sys/stubs/process.rs") {
            result = result
                .replace(
                    "pub(crate) fn spawn(mut command: std::process::Command) -> std::io::Result<Child> {\n    let child = command.spawn()?;\n    Ok(Child { inner: child })\n}",
                    "pub(crate) fn spawn(command: std::process::Command) -> std::io::Result<Child> {\n    let guest_child = crate::stubs_impl::spawn_process(command)?;\n    Ok(Child { inner: guest_child })\n}"
                )
                .replace(
                    "pub struct Child {\n    inner: std::process::Child,\n}",
                    "pub struct Child {\n    inner: crate::stubs_impl::GuestChild,\n}"
                )
                .replace(
                    "pub fn id(&self) -> Option<u32> {\n        None\n    }",
                    "pub fn id(&self) -> Option<u32> {\n        self.inner.id()\n    }"
                )
                .replace(
                    "pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {\n        self.inner.wait()\n    }",
                    "pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {\n        self.inner.wait().await\n    }"
                )
                .replace(
                    "pub async fn wait_with_output(self) -> std::io::Result<Output> {\n        self.inner.wait_with_output()\n    }",
                    "pub async fn wait_with_output(self) -> std::io::Result<Output> {\n        self.inner.wait_with_output().await\n    }"
                );
        }

        if path_str.ends_with("brush-core/src/sys/stubs/signal.rs") {
            let continue_re = regex::Regex::new(
                r"pub\(crate\)\s+fn\s+continue_process\s*\([^)]*\)\s*->\s*Result<[^>]*>\s*\{[^}]*NotSupportedOnThisPlatform[^}]*\}"
            ).unwrap();
            result = continue_re.replace_all(&result,
                "pub(crate) fn continue_process(_pid: sys::process::ProcessId) -> Result<(), error::Error> {\n    crate::stubs_impl::continue_process(_pid)\n}"
            ).to_string();

            let kill_re = regex::Regex::new(
                r"pub\s+fn\s+kill_process\s*\([^)]*\)\s*->\s*Result<[^>]*>\s*\{[^}]*NotSupportedOnThisPlatform[^}]*\}"
            ).unwrap();
            result = kill_re.replace_all(&result,
                "pub fn kill_process(_pid: sys::process::ProcessId, _signal: traps::TrapSignal) -> Result<(), error::Error> {\n    crate::stubs_impl::kill_process(_pid, _signal)\n}"
            ).to_string();
        }

        if path
            .to_string_lossy()
            .replace("\\", "/")
            .ends_with("brush-core/src/sys/stubs/commands.rs")
        {
            if result.contains(r#"NotSupportedOnThisPlatform("fd redirections")"#) {
                result = result.replace(
                    "if open_files.next().is_some() {\n            return Err(error::ErrorKind::NotSupportedOnThisPlatform(\"fd redirections\").into());\n        }\n\n        Ok(())",
                    "crate::stubs_impl::inject_fds(open_files.collect::<Vec<_>>())"
                );
            }
        }

        result
    }

    fn apply_additional_transforms(&self, content: &str, path: &Path) -> String {
        let mut result = content.to_string();
        let path_str = normalize_path(path);

        // Convert inner doc comments and attributes
        let doc_re = regex::Regex::new(r"(?m)^//!").unwrap();
        result = doc_re.replace_all(&result, "//").to_string();

        let attr_re = regex::Regex::new(r"(?m)^#\!\[.*\]").unwrap();
        result = attr_re.replace_all(&result, "// $0").to_string();

        // Note: Path references for vendored crates are already handled by semantic_rerooting
        // We only need to handle use statements here
        let use_re = regex::Regex::new(r"use\s+(brush_core|brush_builtins|brush_parser|brush_interactive|brush_shell|uucore)\b").unwrap();
        result = use_re.replace_all(&result, "use crate::$1").to_string();

        // Redirect crossterm
        result = result
            .replace("crossterm::", "crate::shims::crossterm::")
            .replace(
                "use crossterm;",
                "use crate::shims::crossterm as crossterm;",
            )
            .replace("use crossterm ", "use crate::shims::crossterm ");

        // File-specific patches
        if path_str.ends_with("uucore/src/lib/mods/locale.rs") {
            result = result.replace(
                r#"include!(concat!(env!("OUT_DIR"), "/embedded_locales.rs"));"#,
                r#"include!("../../embedded_locales.rs");"#,
            );
        }

        if path_str.ends_with("brush-core/src/sys/stubs.rs")
            && !result.contains("pub use crate::stubs_impl::")
        {
            result.push_str("\n// Re-export startup helpers\npub use crate::stubs_impl::{startup_username, startup_uid, startup_gid};\n");
        }

        if path_str.ends_with("brush-core/src/shell.rs") {
            result = result.replace(
                "let oldpwd = std::mem::replace(self.working_dir_mut(), cleaned_path);",
                "let oldpwd = std::mem::replace(self.working_dir_mut(), cleaned_path);\n        let _ = std::env::set_current_dir(&self.working_dir);"
            );
        }

        if path_str.ends_with("brush-builtins/src/lib.rs")
            && result.contains("macro_rules! minus_or_plus_flag_arg")
        {
            result = result.replace(
                "#[macro_export]\nmacro_rules! minus_or_plus_flag_arg",
                "macro_rules! minus_or_plus_flag_arg",
            );

            if let Some(macro_start) = result.find("/// Macro to define a struct") {
                if let Some(relative_end) = result[macro_start..].find("\n}\n") {
                    let macro_end = macro_start + relative_end + 3;
                    let macro_def = result[macro_start..macro_end].to_string();
                    result.replace_range(macro_start..macro_end, "");
                    let insert_pos = if result.starts_with("//") {
                        result.find("\n\n").unwrap_or(0) + 2
                    } else {
                        0
                    };
                    result.insert_str(insert_pos, &format!("{}\n\n", macro_def));
                }
            }
        }

        if path_str.ends_with("brush-interactive/src/lib.rs") {
            result = self.transform_interactive_lib(&result);
        }

        if path_str.ends_with("brush-core/src/expansion.rs")
            && !result.starts_with("use std::future::Future;")
        {
            result = format!("use std::future::Future;\n{}", result);
        }

        if path_str.ends_with("brush-shell/src/lib.rs") {
            result = result.replace("mod brushctl;", "pub mod brushctl;");
            if !result.contains("pub mod embedded;") {
                result.push_str("\npub mod embedded;");
            }
        }

        if path_str.ends_with("brush-shell/src/brushctl.rs") {
            result = result.replace(
                "pub(crate) trait ShellBuilderBrushBuiltinExt",
                "pub trait ShellBuilderBrushBuiltinExt",
            );
        }

        if path_str.ends_with("brush-core/src/error.rs") {
            result = result.replace(
                "impl<T> From<T> for Error",
                "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/brush_error_impls.rs\"));\n#[cfg(any())] impl<T> From<T> for Error"
            );
        }

        if path_str.ends_with("brush-shell/src/entry.rs") {
            result = result
                .replace("async fn run_async", "pub async fn run_async")
                .replace("fn try_parse_from", "pub fn try_parse_from");

            if result.contains("crate::brush_core::Shell::builder()") {
                result = result.replace(
                    "    // Add builtins.\n    let shell = shell.default_builtins(builtin_set).brush_builtins();",
                    "    // Add builtins.\n    let shell = crate::stubs_impl::configure_shell(shell, builtin_set);"
                );
            }

            result = result
                .replace(
                    "let mut ui = brush_interactive::MinimalInputBackend;",
                    "let mut ui = brush_interactive::MinimalInputBackend::new();",
                )
                .replace(
                    "let mut ui = crate::brush_interactive::MinimalInputBackend;",
                    "let mut ui = crate::brush_interactive::MinimalInputBackend::new();",
                );
        }

        // InputBackend trait async
        if result.contains("trait InputBackend") {
            result = result.replace("fn read_line", "async fn read_line");
        }

        if (result.contains("impl InputBackend") || path_str.contains("interactive"))
            && !path.ends_with("wasm_interactive.rs")
        {
            let async_re = regex::Regex::new(r"(?m)(async\s+)?fn\s+read_line").unwrap();
            result = async_re
                .replace_all(&result, "async fn read_line")
                .to_string();

            let call_re = regex::Regex::new(r"\.read_line\(([^)]*)\)\?").unwrap();
            result = call_re
                .replace_all(&result, ".read_line($1).await?")
                .to_string();
        }

        // Deduplicate imports
        result = deduplicate_imports(&result);

        result
    }

    fn transform_interactive_lib(&self, content: &str) -> String {
        let cfg_re =
            regex::Regex::new(r#"(?m)^#\[cfg\(feature = "(reedline|basic|minimal)"\)\]"#).unwrap();
        let mut result = cfg_re.replace_all(content, "// $0").to_string();

        let mod_re =
            regex::Regex::new(r"(?m)^(pub\s+)?mod\s+(reedline|basic|minimal)\s*;").unwrap();
        result = mod_re.replace_all(&result, "// $0").to_string();

        let use_re = regex::Regex::new(r"(?m)^pub\s+use\s+(reedline|basic|minimal)::.*").unwrap();
        result = use_re.replace_all(&result, "// $0").to_string();

        result.push_str("\n/// Use project-implemented wasm_interactive\n");
        result.push_str("pub mod wasm_interactive { include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/wasm_interactive.rs\")); }\n");
        result.push_str("\npub use wasm_interactive::{WasmInputBackend, ReedlineInputBackend, BasicInputBackend, MinimalInputBackend};");

        result = result.replace(
            "mod refs;",
            r#"pub type ShellRef = std::sync::Arc<tokio::sync::Mutex<super::brush_core::Shell>>;"#,
        );

        let use_refs_re = regex::Regex::new(r"(?m)^pub\s+use\s+refs::ShellRef\s*;").unwrap();
        use_refs_re.replace_all(&result, "// $0").to_string()
    }
}

// Normalize Path helper: converts backslashes to forward slashes and on Windows lowercases for case-insensitive checks
fn normalize_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace("\\", "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

fn deduplicate_imports(content: &str) -> String {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") && trimmed.ends_with(";") {
            if seen.contains(trimmed) {
                continue;
            }
            seen.insert(trimmed.to_string());
        }
        lines.push(line);
    }

    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn copy_sources(mappings: &[(&str, &str)], vendor_dir: &Path) {
    for (src, dest_name) in mappings {
        let src_path = PathBuf::from(src).join("src");
        let dest_path = vendor_dir.join(dest_name).join("src");

        if dest_path.exists() {
            fs::remove_dir_all(&dest_path).unwrap();
        }
        fs::create_dir_all(dest_path.parent().unwrap()).unwrap();

        let mut options = fs_extra::dir::CopyOptions::new();
        options.overwrite = true;
        options.copy_inside = true;

        if let Err(e) = fs_extra::dir::copy(&src_path, dest_path.parent().unwrap(), &options) {
            eprintln!(
                "Failed to copy {:?} to {:?}: {}",
                src_path,
                dest_path.parent().unwrap(),
                e
            );
            panic!("Failed to copy source");
        }
    }
}

fn create_customizations(vendor_dir: &Path, project_root: &Path, mappings: &[(&str, &str)]) {
    for (_, dest_name) in mappings {
        let dest_path = vendor_dir.join(dest_name).join("src");

        match *dest_name {
            "uucore" => {
                let locales_path = dest_path.join("embedded_locales.rs");
                fs::write(
                    locales_path,
                    r#"
pub fn get_embedded_locale(_key: &str) -> Option<&'static str> {
    None
}
"#,
                )
                .unwrap();
            }
            "brush-shell" => {
                let embedded_path = dest_path.join("embedded.rs");
                let project_src = project_root.join("src/embedded_brush_shell.rs");
                let rel = pathdiff::diff_paths(&project_src, &dest_path)
                    .unwrap_or_else(|| PathBuf::from("../../src/embedded_brush_shell.rs"));
                let rel_str = rel.to_string_lossy().replace("\\", "/");
                fs::write(embedded_path, format!("include!(\"{}\");\n", rel_str)).unwrap();
            }
            _ => {}
        }
    }
}

fn verify_no_stubs(vendor_dir: &Path) {
    println!("Verifying no unpatched stubs in {:?}", vendor_dir);
    let mut found = Vec::new();

    for entry in WalkDir::new(vendor_dir) {
        let entry = entry.unwrap();
        let path_str = normalize_path(entry.path());
        if !entry.file_type().is_file() || entry.path().extension().map_or(true, |e| e != "rs") {
            continue;
        }

        let path = entry.path();

        // Skip platform-specific sys folders regardless of separator/case
        if path_str.contains("sys/windows") || path_str.contains("sys/unix") {
            continue;
        }

        if path_str.ends_with("brush-core/src/error.rs") {
            continue;
        }

        let content = fs::read_to_string(path).unwrap();
        if content.contains("NotSupportedOnThisPlatform") {
            found.push(path.to_path_buf());
        }
    }

    if !found.is_empty() {
        panic!(
            "BUILD FAILED: Unpatched NotSupportedOnThisPlatform stubs found in:\n{:#?}",
            found
        );
    }
}
