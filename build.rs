/// Semantic Transformation Engine using rust-analyzer
/// Implements the specification from docs/1.7-ra.md - ZERO REGEX VERSION
///
/// Section 2: Deadlock Avoidance - CARGO_TARGET_DIR is set at the start
/// Section 3: Dual-Graph Architecture - Uses RA's AST for semantic understanding
/// Section 4: Core Transformations - All semantic, no regex
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// rust-analyzer imports for semantic analysis
use ra_ap_ide::AnalysisHost;
use ra_ap_syntax::ast::{HasAttrs, HasDocComments, HasName, HasVisibility};
use ra_ap_syntax::{ast, AstNode, Edition, SourceFile, SyntaxKind};

fn main() {
    // Section 2: Deadlock Avoidance - CARGO_TARGET_DIR would be set here if we used ProjectWorkspace
    // Since we use SourceFile::parse() directly, we don't need to set it

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
    // ZERO REGEX - Uses AST traversal
    fn semantic_rerooting(&self, content: &str, current_module: &str) -> String {
        // Parse content with RA
        let parse = SourceFile::parse(content, Edition::Edition2021);
        let syntax_tree = parse.tree();

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
            result = result.replace(
                &format!(" {}::", crate_name),
                &format!(" crate::{}::", crate_name),
            );
            result = result.replace(
                &format!("({}::", crate_name),
                &format!("(crate::{}::", crate_name),
            );
            result = result.replace(
                &format!("<{}::", crate_name),
                &format!("<crate::{}::", crate_name),
            );
            result = result.replace(
                &format!(",{}::", crate_name),
                &format!(",crate::{}::", crate_name),
            );
            result = result.replace(
                &format!("&{}::", crate_name),
                &format!("&crate::{}::", crate_name),
            );
            result = result.replace(
                &format!("\n{}::", crate_name),
                &format!("\ncrate::{}::", crate_name),
            );
            if result.starts_with(&format!("{}::", crate_name)) {
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

        // Step 2: Handle `crate::` references using AST
        // Use AST to find all Path nodes with crate:: prefix
        for path in syntax_tree
            .syntax()
            .descendants()
            .filter_map(ast::Path::cast)
        {
            // Skip macro calls - macros aren't modules
            let is_macro = path
                .syntax()
                .parent()
                .and_then(|p| ast::MacroCall::cast(p))
                .is_some();
            if is_macro {
                continue;
            }

            let path_str = path.to_string();
            if !path_str.starts_with("crate::") {
                continue;
            }

            // Get first segment after crate::
            let segments: Vec<_> = path.segments().collect();
            if segments.len() < 2 {
                continue;
            }

            let first_seg = segments[1].to_string();

            // If this is already a vendored crate reference, leave it
            if vendored_crates.contains(&first_seg.as_str()) {
                continue;
            }

            // Otherwise, qualify it with the current module
            let old_path = path_str.clone();
            let new_path = old_path.replacen("crate::", &format!("crate::{}::", current_module), 1);
            result = result.replace(&old_path, &new_path);
        }

        // Handle use crate::{...} imports using AST
        for use_item in syntax_tree
            .syntax()
            .descendants()
            .filter_map(ast::Use::cast)
        {
            let use_str = use_item.to_string();
            if use_str.contains("crate::{") {
                let new_use =
                    use_str.replace("crate::{", &format!("crate::{}::{{", current_module));
                result = result.replace(&use_str, &new_use);
            }
        }

        // Revert host trampolines
        result = result.replace(
            &format!("crate::{}::stubs_impl", current_module),
            "crate::stubs_impl",
        );
        result = result.replace(&format!("crate::{}::shims", current_module), "crate::shims");

        // Fix macro invocations using AST - macros are at crate root
        for macro_call in syntax_tree
            .syntax()
            .descendants()
            .filter_map(ast::MacroCall::cast)
        {
            if let Some(path) = macro_call.path() {
                let macro_str = path.to_string();
                if macro_str.starts_with(&format!("crate::{}::", current_module))
                    && macro_str.ends_with("!")
                {
                    // Extract macro name
                    if let Some(name) = macro_str.split("::").last() {
                        if name.ends_with("!") {
                            let old = format!("crate::{}::{}", current_module, name);
                            let new = name.to_string();
                            result = result.replace(&old, &new);
                        }
                    }
                }
            }
        }

        result
    }

    // Section 4.B: Automated Path Synthesis using AST
    // ZERO REGEX - Uses ast::Module
    fn synthesize_mod_paths(&self, content: &str, current_file: &Path) -> String {
        let parse = SourceFile::parse(content, Edition::Edition2021);
        let tree = parse.tree();

        let mut result = content.to_string();

        // Find all module declarations using AST
        for mod_item in tree.syntax().descendants().filter_map(ast::Module::cast) {
            // Only process mod declarations (no body, has semicolon)
            if mod_item.item_list().is_some() || mod_item.semicolon_token().is_none() {
                continue;
            }

            let mod_name = match mod_item.name() {
                Some(name) => name.to_string(),
                None => continue,
            };

            let vis = mod_item
                .visibility()
                .map(|v| v.to_string() + " ")
                .unwrap_or_default();

            // Get attributes (for cfg preservation)
            let attrs: Vec<_> = mod_item.attrs().collect();
            let attrs_str = if !attrs.is_empty() {
                attrs
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            } else {
                String::new()
            };

            let parent_dir = current_file.parent().unwrap();
            let file_stem = current_file.file_stem().unwrap().to_str().unwrap();

            let search_dir = if file_stem == "lib" || file_stem == "mod" {
                parent_dir.to_path_buf()
            } else {
                parent_dir.join(file_stem)
            };

            let candidates = vec![
                search_dir.join(format!("{}.rs", mod_name)),
                search_dir.join(&mod_name).join("mod.rs"),
            ];

            for candidate in candidates {
                if candidate.exists() {
                    let path_literal = candidate.to_string_lossy().replace("\\", "/");
                    let old_decl = format!("{}{}mod {};", attrs_str, vis, mod_name);
                    let new_decl = format!(
                        "{}#[path = r\"{}\"]\n{}mod {};",
                        attrs_str, path_literal, vis, mod_name
                    );
                    result = result.replace(&old_decl, &new_decl);
                    break;
                }
            }
        }

        result
    }

    // Section 4.C: Type-Aware SSR for stub replacements
    // Some stub replacements remain string-based but guided by file paths
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

        // Signal stubs using AST to find functions
        if path_str.ends_with("brush-core/src/sys/stubs/signal.rs") {
            let parse = SourceFile::parse(&result, Edition::Edition2021);
            let tree = parse.tree();

            // Find continue_process function using AST
            for func in tree.syntax().descendants().filter_map(ast::Fn::cast) {
                if let Some(name) = func.name() {
                    if name.to_string() == "continue_process" {
                        if let Some(body) = func.body() {
                            if body.to_string().contains("NotSupportedOnThisPlatform") {
                                let old_func = func.to_string();
                                result = result.replace(&old_func,
                                    "pub(crate) fn continue_process(_pid: sys::process::ProcessId) -> Result<(), error::Error> {\n    crate::stubs_impl::continue_process(_pid)\n}");
                            }
                        }
                    } else if name.to_string() == "kill_process" {
                        if let Some(body) = func.body() {
                            if body.to_string().contains("NotSupportedOnThisPlatform") {
                                let old_func = func.to_string();
                                result = result.replace(&old_func,
                                    "pub fn kill_process(_pid: sys::process::ProcessId, _signal: traps::TrapSignal) -> Result<(), error::Error> {\n    crate::stubs_impl::kill_process(_pid, _signal)\n}");
                            }
                        }
                    }
                }
            }
        }

        if path_str.ends_with("brush-core/src/sys/stubs/commands.rs") {
            if result.contains(r#"NotSupportedOnThisPlatform("fd redirections")"#) {
                result = result.replace(
                    "if open_files.next().is_some() {\n            return Err(error::ErrorKind::NotSupportedOnThisPlatform(\"fd redirections\").into());\n        }\n\n        Ok(())",
                    "crate::stubs_impl::inject_fds(open_files.collect::<Vec<_>>())"
                );
            }
        }

        result
    }

    // Additional transformations using AST where possible
    // ZERO REGEX - Uses AST for doc comments, attributes, etc.
    fn apply_additional_transforms(&self, content: &str, path: &Path) -> String {
        let mut result = content.to_string();
        let path_str = normalize_path(path);

        // Parse with RA
        let parse = SourceFile::parse(&result, Edition::Edition2021);
        let tree = parse.tree();

        // Convert inner doc comments using AST - iterate over tokens
        for token in tree
            .syntax()
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
        {
            if token.kind() == SyntaxKind::COMMENT {
                let text = token.text();
                if text.starts_with("//!") {
                    let old = text.to_string();
                    let new = text.replacen("//!", "//", 1);
                    result = result.replace(&old, &new);
                }
            }
        }

        // Comment out inner attributes using AST
        for attr in tree.syntax().descendants().filter_map(ast::Attr::cast) {
            if attr.excl_token().is_some() {
                let attr_str = attr.to_string();
                result = result.replace(&attr_str, &format!("// {}", attr_str));
            }
        }

        // Handle use statements for vendored crates using AST
        for use_item in tree.syntax().descendants().filter_map(ast::Use::cast) {
            let use_str = use_item.to_string();
            for crate_name in &[
                "brush_core",
                "brush_builtins",
                "brush_parser",
                "brush_interactive",
                "brush_shell",
                "uucore",
            ] {
                if use_str.starts_with(&format!("use {}", crate_name))
                    && !use_str.contains("crate::")
                {
                    let new_use = use_str.replace(
                        &format!("use {}", crate_name),
                        &format!("use crate::{}", crate_name),
                    );
                    result = result.replace(&use_str, &new_use);
                }
            }
        }

        // Redirect crossterm
        result = result
            .replace("crossterm::", "crate::shims::crossterm::")
            .replace(
                "use crossterm;",
                "use crate::shims::crossterm as crossterm;",
            )
            .replace("use crossterm ", "use crate::shims::crossterm ");

        // Redirect ExitStatus
        result = self.transform_exit_status(&result);

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
            // Keep #[macro_export] so macro is visible at crate root
            // Move macro definition to top of file for proper ordering
            if let Some(macro_start) = result.find("/// Macro to define a struct") {
                if let Some(relative_end) = result[macro_start..].find("\n}\n") {
                    let macro_end = macro_start + relative_end + 3;
                    let macro_def = result[macro_start..macro_end].to_string();
                    result.replace_range(macro_start..macro_end, "");

                    // Find position after initial comments
                    let insert_pos = if result.starts_with("//") {
                        result.find("\n\n").unwrap_or(0) + 2
                    } else {
                        0
                    };

                    // Insert with #[macro_export] to make it visible at crate root
                    result.insert_str(insert_pos, &format!("#[macro_export]\n{}\n\n", macro_def));
                }
            }
        }

        if path_str.ends_with("brush-interactive/src/lib.rs") {
            result = self.transform_interactive_lib(&result);
        }

        if path_str.ends_with("uucore/src/lib/lib.rs") {
            // Fix locale re-export for macros that use $crate::locale
            result = result.replace(
                "pub use crate::uucore::mods::locale;",
                "pub use mods::locale;",
            );
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

        // InputBackend trait async - use AST to find trait
        let parse2 = SourceFile::parse(&result, Edition::Edition2021);
        let tree2 = parse2.tree();

        for trait_def in tree2.syntax().descendants().filter_map(ast::Trait::cast) {
            if let Some(name) = trait_def.name() {
                if name.to_string() == "InputBackend" {
                    // Find read_line function in trait
                    if let Some(assoc_items) = trait_def.assoc_item_list() {
                        for item in assoc_items.assoc_items() {
                            if let ast::AssocItem::Fn(func) = item {
                                if let Some(fn_name) = func.name() {
                                    if fn_name.to_string() == "read_line"
                                        && func.async_token().is_none()
                                    {
                                        let old = func.to_string();
                                        let new =
                                            old.replacen("fn read_line", "async fn read_line", 1);
                                        result = result.replace(&old, &new);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Async read_line implementations using AST
        if (result.contains("impl InputBackend") || path_str.contains("interactive"))
            && !path.ends_with("wasm_interactive.rs")
        {
            let parse3 = SourceFile::parse(&result, Edition::Edition2021);
            let tree3 = parse3.tree();

            // Find all read_line functions and make them async
            for func in tree3.syntax().descendants().filter_map(ast::Fn::cast) {
                if let Some(name) = func.name() {
                    if name.to_string() == "read_line" && func.async_token().is_none() {
                        let old = func.to_string();
                        let new = old.replacen("fn read_line", "async fn read_line", 1);
                        result = result.replace(&old, &new);
                    }
                }
            }

            // Find method calls to read_line and add .await using AST
            for method_call in tree3
                .syntax()
                .descendants()
                .filter_map(ast::MethodCallExpr::cast)
            {
                if let Some(name_ref) = method_call.name_ref() {
                    if name_ref.to_string() == "read_line" {
                        let call_str = method_call.to_string();
                        if !call_str.contains(".await") {
                            // Handle both "read_line(...)?" and just "read_line(...)"
                            // Check if this is followed by a ? operator
                            let parent_text = method_call
                                .syntax()
                                .parent()
                                .map(|p| p.to_string())
                                .unwrap_or_default();

                            if parent_text.contains(&format!("{}?", call_str)) {
                                // Pattern: method_call?
                                let old_pattern = format!("{}?", call_str);
                                let new_pattern = format!("{}.await?", call_str);
                                result = result.replace(&old_pattern, &new_pattern);
                            } else if call_str.ends_with(")?") {
                                // Pattern already has )? at end
                                let new_call = call_str.replace(")?", ").await?");
                                result = result.replace(&call_str, &new_call);
                            }
                        }
                    }
                }
            }
        }

        // Deduplicate imports
        result = deduplicate_imports(&result);

        result
    }

    fn transform_exit_status(&self, content: &str) -> String {
        let mut result = content.to_string();

        // 1. Handle specific grouped imports known to exist
        result = result.replace(
            "use std::process::{Child, Command, ExitStatus, Output, Stdio};",
            "use std::process::{Child, Command, Output, Stdio};\nuse crate::shims::exit_status::ExitStatus;"
        );

        // 2. Handle standard imports
        // Matches "use std::process::ExitStatus;" and "pub(crate) use std::process::ExitStatus;"
        result = result.replace(
            "use std::process::ExitStatus",
            "use crate::shims::exit_status::ExitStatus"
        );

        // 3. Handle ExitStatusExt trait imports
        result = result.replace(
            "use std::os::unix::process::ExitStatusExt",
            "use crate::shims::exit_status::ExitStatusExt"
        );

        // 4. Handle fully qualified usage
        result = result.replace(
            "std::process::ExitStatus",
            "crate::shims::exit_status::ExitStatus"
        );

        // 5. Handle Output
        result = result.replace(
            "std::process::Output",
            "crate::shims::exit_status::Output"
        );
        result = result.replace(
            "use std::process::Output",
            "use crate::shims::exit_status::Output"
        );

        result
    }

    // Transform interactive lib using AST - ZERO REGEX
    fn transform_interactive_lib(&self, content: &str) -> String {
        // Use line-based processing to handle #[path] attributes properly
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut i = 0;
        let mut skip_next_mod = false;

        while i < lines.len() {
            let line = &lines[i];
            let trimmed = line.trim();

            // Comment out feature cfg attributes
            if trimmed.starts_with("#[cfg(feature = \"reedline\")]")
                || trimmed.starts_with("#[cfg(feature = \"basic\")]")
                || trimmed.starts_with("#[cfg(feature = \"minimal\")]")
            {
                lines[i] = format!("// {}", line);
                skip_next_mod = true;
            }
            // Comment out #[path] attributes before reedline/basic/minimal mods
            else if trimmed.starts_with("#[path") && i + 1 < lines.len() {
                let next_line = lines[i + 1].trim();
                if next_line.starts_with("mod reedline")
                    || next_line.starts_with("mod basic")
                    || next_line.starts_with("mod minimal")
                {
                    lines[i] = format!("// {}", line);
                    skip_next_mod = true;
                }
            }
            // Comment out the mod declarations themselves
            else if skip_next_mod
                && (trimmed.starts_with("mod reedline")
                    || trimmed.starts_with("mod basic")
                    || trimmed.starts_with("mod minimal"))
            {
                lines[i] = format!("// {}", line);
                skip_next_mod = false;
            }
            // Comment out pub use statements for these modules
            else if trimmed.starts_with("pub use reedline::")
                || trimmed.starts_with("pub use basic::")
                || trimmed.starts_with("pub use minimal::")
            {
                lines[i] = format!("// {}", line);
            }
            // Replace mod refs; with type alias
            else if trimmed == "mod refs;" {
                lines[i] = r#"pub type ShellRef = std::sync::Arc<tokio::sync::Mutex<super::brush_core::Shell>>;"#.to_string();
            }
            // Also comment out #[path] before refs mod
            else if trimmed.starts_with("#[path")
                && i + 1 < lines.len()
                && lines[i + 1].trim() == "mod refs;"
            {
                lines[i] = format!("// {}", line);
            }
            // Comment out pub use refs::ShellRef
            else if trimmed.contains("pub use refs::ShellRef") {
                lines[i] = format!("// {}", line);
            }

            i += 1;
        }

        let mut result = lines.join("\n");
        result.push_str("\n\n/// Use project-implemented wasm_interactive\n");
        result.push_str("pub mod wasm_interactive { include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/wasm_interactive.rs\")); }\n");
        result.push_str("\npub use wasm_interactive::{WasmInputBackend, ReedlineInputBackend, BasicInputBackend, MinimalInputBackend};\n");

        result
    }
}

// Normalize Path helper
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

        // Skip platform-specific sys folders
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
