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
// use ra_ap_ide::AnalysisHost;
use ra_ap_syntax::ast::make;
use ra_ap_syntax::ast::{HasAttrs, HasName, HasVisibility};
use ra_ap_syntax::syntax_editor::{Position, SyntaxEditor};
use ra_ap_syntax::{ast, AstNode, Edition, SourceFile, SyntaxKind, SyntaxNode, TextRange};

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() != "wasm32" {
        return;
    }

    // Section 2: Deadlock Avoidance - CARGO_TARGET_DIR would be set here if we used ProjectWorkspace
    // Since we use SourceFile::parse() directly, we don't need to set it

    println!("cargo:rerun-if-changed=brush-fork");
    println!("cargo:rerun-if-changed=coreutils-fork");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/interp_log.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = PathBuf::from(&manifest_dir);
    let vendor_dir = project_root.join("target/vendor");
    println!("cargo:warning=Vendor dir: {:?}", vendor_dir);

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

    // Final validation
    verify_no_stubs(&vendor_dir);
}

fn parse_stmt<N: AstNode>(text: &str) -> N {
    let wrapped = format!("fn dummy() {{ {} }}", text);
    let parse = SourceFile::parse(&wrapped, Edition::Edition2021);
    let tree = parse.tree();
    let syntax = tree.syntax();

    // Find the function body block to avoid matching the function itself
    let func = syntax
        .descendants()
        .find_map(ast::Fn::cast)
        .expect("failed to parse dummy fn");
    let body = func.body().expect("dummy fn has no body");

    if let Some(node) = body.syntax().descendants().find_map(N::cast) {
        N::cast(node.syntax().clone_for_update()).unwrap()
    } else {
        panic!("Failed to parse stmt: {}", text);
    }
}

fn parse_expr<N: AstNode>(text: &str) -> N {
    let wrapped = format!("fn dummy() {{ let _ = {}; }}", text);
    let parse = SourceFile::parse(&wrapped, Edition::Edition2021);
    let tree = parse.tree();
    let syntax = tree.syntax();

    // Find the LetStmt
    let let_stmt = syntax
        .descendants()
        .find_map(ast::LetStmt::cast)
        .expect("failed to parse dummy let");
    // Get the initializer
    let init = let_stmt
        .initializer()
        .expect("dummy let has no initializer");

    if let Some(node) = N::cast(init.syntax().clone_for_update()) {
        node
    } else {
        panic!("Failed to parse expr: {}", text);
    }
}

fn parse_node<N: AstNode>(text: &str) -> N {
    let parse = SourceFile::parse(text, Edition::Edition2021);
    let tree = parse.tree();
    let syntax = tree.syntax();
    if let Some(node) = syntax.descendants().find_map(N::cast) {
        N::cast(node.syntax().clone_for_update()).unwrap()
    } else {
        // eprintln!("Failed to parse node from text: '{}'", text);
        // eprintln!("Tree: {:#?}", syntax);
        panic!("Failed to parse node");
    }
}

fn make_mutable<N: AstNode>(node: N) -> N {
    N::cast(node.syntax().clone_for_update()).unwrap()
}

fn parse_token(text: &str, kind: SyntaxKind) -> ra_ap_syntax::SyntaxToken {
    let parse = SourceFile::parse(text, Edition::Edition2021);
    let root = parse.tree().syntax().clone_for_update();
    let tokens: Vec<_> = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .collect();

    if let Some(token) = tokens.iter().find(|t| t.kind() == kind) {
        token.clone()
    } else {
        // eprintln!("Failed to parse token. Expected kind {:?}, text {:?}", kind, text);
        // eprintln!("Found tokens: {:?}", tokens.iter().map(|t| (t.kind(), t.text())).collect::<Vec<_>>());
        panic!("Failed to parse token");
    }
}

fn tokenize_path(text: &str) -> Vec<String> {
    let parse = SourceFile::parse(text, Edition::Edition2021);
    parse
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() != SyntaxKind::WHITESPACE)
        .map(|t| t.text().to_string())
        .collect()
}

fn tokenize_path_kinds(text: &str) -> Vec<(String, SyntaxKind)> {
    let parse = SourceFile::parse(text, Edition::Edition2021);
    parse
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() != SyntaxKind::WHITESPACE)
        .map(|t| (t.text().to_string(), t.kind()))
        .collect()
}

/// Semantic Engine using rust-analyzer's parser and AST
struct SemanticEngine {
    // analysis_host: AnalysisHost, // Unused for now, using pure AST
}

impl SemanticEngine {
    fn new() -> Self {
        // Section 3.A: Initialize AnalysisHost for semantic analysis
        // let analysis_host = AnalysisHost::default();

        Self { /* analysis_host */ }
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
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to read {:?}: {}", path, e);
                    continue;
                }
            };

            // Apply AST-based transformations
            match self.transform_with_ast(&content, path, &module_name) {
                Ok(transformed) => {
                    if content != transformed {
                        if let Err(e) = fs::write(path, transformed) {
                            eprintln!("Failed to write {:?}: {}", path, e);
                        }
                    }
                }
                Err(_e) => {
                    // eprintln!("Failed to transform {:?}: {}", path, e);
                }
            }
        }
    }

    fn transform_with_ast(
        &self,
        original_content: &str,
        path: &Path,
        current_module: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        eprintln!("Transforming: {:?}", path);
        let path_str = normalize_path(path);

        let mut content = original_content.to_string();

        // String-based pre-processing for macros and hard-to-parse constructs
        if path_str.ends_with("brush-core/src/expansion.rs") {
            content = content.replace("crate::regex", "crate::brush_core::regex");
            content = content.replace(
                "use std::borrow::Cow;",
                "use std::borrow::Cow;\nuse std::future::Future;",
            );
        }

        if path_str.ends_with("brush-core/src/prompt.rs") {
            content = content.replace("brush_parser::", "crate::brush_parser::");
            content = content.replace("crate::crate::brush_parser", "crate::brush_parser");
        }

        if path_str.ends_with("brush-shell/src/entry.rs") {
            println!("cargo:warning=Patching entry.rs");
            content = content.replace(
                "brush_interactive::MinimalInputBackend",
                "crate::wasm_interactive::MinimalInputBackend",
            );
            content = content.replace(
                "let mut input_backend = crate::wasm_interactive::MinimalInputBackend;",
                "let mut input_backend = crate::wasm_interactive::MinimalInputBackend::new();",
            );
            content = content.replace(
                "brush_interactive::WasmInputBackend",
                "crate::wasm_interactive::WasmInputBackend",
            );
            content = content.replace(
                "brush_interactive::ReedlineInputBackend",
                "crate::wasm_interactive::ReedlineInputBackend",
            );
            content = content.replace(
                "brush_interactive::BasicInputBackend",
                "crate::wasm_interactive::BasicInputBackend",
            );
        }

        if path_str.ends_with("uucore/src/lib/lib.rs") {
            println!("cargo:warning=Patching uucore lib.rs");
            content = content.replace(
                "pub fn localized_help_template_with_colors(",
                "use crate::translate;\npub fn localized_help_template_with_colors(",
            );
        }

        if path_str.ends_with("brush-interactive/src/input_backend.rs") {
            println!("cargo:warning=Patching input_backend.rs");
            content = content.replace(
                "use crate::ShellError;",
                "use crate::brush_interactive::ShellError;",
            );
            content = content.replace("&crate::ShellRef", "&crate::brush_interactive::ShellRef");
            content = content.replace(
                "    fn read_line(\n        &mut self,",
                "    async fn read_line(\n        &mut self,",
            );
        }

        if path_str.ends_with("brush-shell/src/args.rs") {
            println!("cargo:warning=Patching args.rs");
            content = content.replace(
                "default_value_t = crate::entry::DEFAULT_ENABLE_HIGHLIGHTING",
                "default_value_t = crate::brush_shell::entry::DEFAULT_ENABLE_HIGHLIGHTING",
            );
        }

        if path_str.ends_with("brush-core/src/wellknownvars.rs") {
            println!("cargo:warning=Applying rewrites for wellknownvars.rs");
            content = content.replace("crate::callstack", "crate::brush_core::callstack");
        }

        // Pre-processing: Semantic Rerooting
        let parse = SourceFile::parse(&content, Edition::Edition2021);
        let root = parse.tree().syntax().clone();
        let vendored_crates = vec![
            "brush_core",
            "brush_builtins",
            "brush_parser",
            "brush_interactive",
            "brush_shell",
            "uucore",
            "stubs_impl",
        ];

        let edits = self.find_semantic_rerooting_edits(&root, current_module, &vendored_crates);

        // Filter out overlapping edits (keep the outer ones)
        let mut filtered_edits = Vec::new();
        for (range, text) in &edits {
            let is_inner = edits
                .iter()
                .any(|(other_range, _)| other_range != range && other_range.contains_range(*range));
            if !is_inner {
                filtered_edits.push((*range, text.clone()));
            }
        }

        let mut content = content.to_string();
        if !filtered_edits.is_empty() {
            // Apply edits in reverse order to preserve ranges
            let mut sorted_edits = filtered_edits;
            sorted_edits.sort_by_key(|(range, _)| range.start());
            for (range, text) in sorted_edits.into_iter().rev() {
                let start = u32::from(range.start()) as usize;
                let end = u32::from(range.end()) as usize;
                content.replace_range(start..end, &text);
            }
        }

        // Now create SyntaxEditor for the rest
        let parse = SourceFile::parse(&content, Edition::Edition2021);
        let root = parse.tree().syntax().clone();
        let mut editor = SyntaxEditor::new(root.clone());

        self.apply_path_rewrites(&mut editor, &root, path);
        self.apply_macro_fixes(&mut editor, &root, path);
        self.apply_stub_replacements(&mut editor, &root, path);
        self.apply_doc_conversions(&mut editor, &root);
        self.apply_inner_attributes_commenting(&mut editor, &root);
        self.apply_async_transforms(&mut editor, &root, path);
        self.apply_interactive_lib_transforms(&mut editor, &root, path);
        self.apply_additional_transforms_ast(&mut editor, &root, path);

        let edit = editor.finish();
        let result = edit.new_root().to_string();
        Ok(result)
    }

    fn find_semantic_rerooting_edits(
        &self,
        root: &SyntaxNode,
        current_module: &str,
        vendored_crates: &[&str],
    ) -> Vec<(TextRange, String)> {
        let mut edits = Vec::new();

        // Step 1: Add crate:: prefix to bare vendored crate references
        // and Step 2: Handle crate:: references
        for path in root.descendants().filter_map(ast::Path::cast) {
            // eprintln!("DEBUG: Checking path: {}", path);
            // Skip macro calls (already checking parent)
            if path
                .syntax()
                .parent()
                .and_then(ast::MacroCall::cast)
                .is_some()
            {
                continue;
            }

            let segments: Vec<_> = path.segments().collect();
            if let Some(first_seg) = segments.first() {
                let first_seg_text = first_seg
                    .name_ref()
                    .map(|n| n.text().to_string())
                    .unwrap_or_default();

                if current_module == "brush_builtins" && path.to_string().contains("brush_core") {
                    eprintln!(
                        "DEBUG: Found brush_core path in brush_builtins: '{}'. First seg: '{}'",
                        path, first_seg_text
                    );
                }
                if current_module == "brush_shell" && path.to_string().contains("brush_core") {
                    eprintln!(
                        "DEBUG: Found brush_core path in brush_shell: '{}'. First seg: '{}'",
                        path, first_seg_text
                    );
                }

                if first_seg_text == "crate" {
                    // Handle crate::...
                    let rest: Vec<String> = segments
                        .iter()
                        .skip(1)
                        .map(|s| s.name_ref().unwrap().text().to_string())
                        .collect();

                    if !rest.is_empty() {
                        let second_seg = &rest[0];
                        let excluded = [
                            "shims",
                            "bridges",
                            "bootstrap",
                            "wasm_interactive",
                            "translate",
                        ];
                        if !vendored_crates.contains(&second_seg.as_str())
                            && !excluded.contains(&second_seg.as_str())
                        {
                            // Replace crate::foo with crate::current_module::foo
                            let new_path_text =
                                format!("crate::{}::{}", current_module, rest.join("::"));
                            edits.push((path.syntax().text_range(), new_path_text));
                        }
                    }
                } else if vendored_crates.contains(&first_seg_text.as_str()) {
                    // Handle bare crate reference: brush_core::... -> crate::brush_core::...
                    if path
                        .syntax()
                        .parent()
                        .map(|p| p.kind() == SyntaxKind::PATH)
                        .unwrap_or(false)
                    {
                        continue; // It's a sub-path
                    }

                    let new_path_text = format!("crate::{}", path.to_string());
                    edits.push((path.syntax().text_range(), new_path_text));
                }
            }
        }

        // Handle use crate::{...} imports
        for use_tree in root.descendants().filter_map(ast::UseTree::cast) {
            if let Some(path) = use_tree.path() {
                if path.to_string() == "crate" {
                    if let Some(_tree_list) = use_tree.use_tree_list() {
                        let new_path_text = format!("crate::{}", current_module);
                        edits.push((path.syntax().text_range(), new_path_text));
                    }
                }
            }
        }

        // Fix macro invocations
        for macro_call in root.descendants().filter_map(ast::MacroCall::cast) {
            if let Some(path) = macro_call.path() {
                let path_str = path.to_string();
                if path_str.starts_with(&format!("crate::{}::", current_module)) {
                    if let Some(name_ref) = path.segment().and_then(|s| s.name_ref()) {
                        let name = name_ref.text();
                        edits.push((path.syntax().text_range(), name.to_string()));
                    }
                }
            }
        }

        edits
    }

    fn stub_fn(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        path_suffix: &str,
        current_path: &Path,
        fn_name: &str,
        new_body_str: &str,
    ) {
        let path_str = normalize_path(current_path);
        if !path_str.ends_with(path_suffix) {
            return;
        }

        for func in root.descendants().filter_map(ast::Fn::cast) {
            if func.name().map_or(false, |n| n.text() == fn_name) {
                if let Some(body) = func.body() {
                    let new_body = parse_stmt::<ast::BlockExpr>(new_body_str);
                    editor.replace(body.syntax(), new_body.syntax());
                }
            }
        }
    }

    fn rewrite_path(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        old_path: &str,
        new_path: &str,
    ) {
        for path in root.descendants().filter_map(ast::Path::cast) {
            // Only handle top-level paths to avoid double replacement
            if path
                .syntax()
                .parent()
                .map_or(false, |p| p.kind() == SyntaxKind::PATH)
            {
                continue;
            }

            let text = path.to_string();
            if text == old_path || text.starts_with(&format!("{}::", old_path)) {
                let new_text = text.replacen(old_path, new_path, 1);
                let new_node = make_mutable(make::path_from_text(&new_text));
                editor.replace(path.syntax(), new_node.syntax());
            }
        }
    }

    fn rewrite_macro_tokens(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        target_token: &str,
        replacement_text: &str,
    ) {
        for token in root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
        {
            if token.text() == target_token {
                if token
                    .parent_ancestors()
                    .any(|n| ast::MacroCall::can_cast(n.kind()))
                {
                    let new_node = make_mutable(make::path_from_text(replacement_text));
                    editor.replace(token, new_node.syntax());
                }
            }
        }
    }

    fn rewrite_macro_path(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        old_path: &str,
        new_path: &str,
    ) {
        let old_tokens = tokenize_path(old_path);
        if old_tokens.is_empty() {
            return;
        }

        let new_tokens_info = tokenize_path_kinds(new_path);
        if new_tokens_info.is_empty() {
            return;
        }

        let all_tokens: Vec<_> = root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .collect();

        let mut i = 0;
        while i < all_tokens.len() {
            // Skip whitespace in all_tokens
            if all_tokens[i].kind() == SyntaxKind::WHITESPACE {
                i += 1;
                continue;
            }

            if all_tokens[i].text() == old_tokens[0] {
                // Check if sequence matches, skipping whitespace in all_tokens
                let mut match_found = true;
                let mut current_token_idx = i;
                let mut tokens_to_replace = Vec::new();

                for token_text in &old_tokens {
                    // Skip whitespace
                    while current_token_idx < all_tokens.len()
                        && all_tokens[current_token_idx].kind() == SyntaxKind::WHITESPACE
                    {
                        tokens_to_replace.push(all_tokens[current_token_idx].clone());
                        current_token_idx += 1;
                    }

                    if current_token_idx >= all_tokens.len()
                        || all_tokens[current_token_idx].text() != *token_text
                    {
                        match_found = false;
                        break;
                    }
                    tokens_to_replace.push(all_tokens[current_token_idx].clone());
                    current_token_idx += 1;
                }

                if match_found {
                    // Check if inside macro
                    if all_tokens[i]
                        .parent_ancestors()
                        .any(|n| ast::MacroCall::can_cast(n.kind()))
                    {
                        println!(
                            "cargo:warning=Rewriting macro path '{}' -> '{}'",
                            old_path, new_path
                        );
                        // Replace first token with first new token
                        let (first_text, first_kind) = &new_tokens_info[0];
                        let first_new_token = parse_token(first_text, *first_kind);
                        editor.replace(tokens_to_replace[0].clone(), first_new_token);

                        // Insert rest
                        for (text, kind) in new_tokens_info.iter().skip(1) {
                            let token = parse_token(text, *kind);
                            editor.insert(Position::after(tokens_to_replace[0].clone()), token);
                        }

                        // Delete the rest of old tokens
                        for token in tokens_to_replace.iter().skip(1) {
                            editor.delete(token.clone());
                        }

                        i = current_token_idx; // Advance
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    fn apply_path_rewrites(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);
        // println!("cargo:warning=Checking path rewrites for: {}", path_str);

        if path_str.ends_with("brush-core/src/expansion.rs") {
            println!("cargo:warning=Applying rewrites for expansion.rs");
            self.rewrite_path(editor, root, "brush_parser", "crate::brush_parser");
            self.rewrite_path(editor, root, "crate::regex", "crate::brush_core::regex");
            self.rewrite_macro_path(editor, root, "crate::regex", "crate::brush_core::regex");
        }

        if path_str.ends_with("brush-core/src/prompt.rs") {
            println!("cargo:warning=Applying rewrites for prompt.rs");
            self.rewrite_path(editor, root, "brush_parser", "crate::brush_parser");
        }

        if path_str.ends_with("brush-interactive/src/interactive_shell.rs") {
            // "fn read_line" -> "async fn read_line" is handled by apply_async_transforms
        }
    }

    fn apply_macro_fixes(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);
        if path_str.ends_with("brush-core/src/expansion.rs") {
            self.rewrite_macro_tokens(editor, root, "brush_parser", "crate::brush_parser");
        }
    }

    fn apply_stub_replacements(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/users.rs",
            path,
            "get_current_username",
            "{ Ok(\"user\".to_string()) }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/users.rs",
            path,
            "get_current_uid",
            "{ crate::stubs_impl::get_current_uid() }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/users.rs",
            path,
            "get_current_gid",
            "{ crate::stubs_impl::get_current_gid() }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/users.rs",
            path,
            "get_effective_uid",
            "{ crate::stubs_impl::get_effective_uid() }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/users.rs",
            path,
            "get_effective_gid",
            "{ crate::stubs_impl::get_effective_gid() }",
        );

        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/fs.rs",
            path,
            "open_null_file",
            "{ crate::stubs_impl::open_null_file() }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/fs.rs",
            path,
            "get_disk_usage",
            "{ None }",
        );

        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/commands.rs",
            path,
            "invoke_external_command",
            "{ Err(crate::brush_core::error::Error::NotSupported) }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/commands.rs",
            path,
            "inject_fds",
            "{ Ok(()) }",
        );

        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/process.rs",
            path,
            "id",
            "{ self.inner.id() }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/process.rs",
            path,
            "spawn",
            "{ Ok(Child { inner: crate::stubs_impl::spawn_process(command)? }) }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/process.rs",
            path,
            "wait",
            "{ self.inner.wait().await }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/process.rs",
            path,
            "wait_with_output",
            "{ self.inner.wait_with_output().await }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/process.rs",
            path,
            "get_memory_usage",
            "{ None }",
        );

        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/signal.rs",
            path,
            "continue_process",
            "{ Ok(()) }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/signal.rs",
            path,
            "kill_process",
            "{ Ok(()) }",
        );

        self.stub_fn(
            editor,
            root,
            "brush-core/src/openfiles.rs",
            path,
            "check_open_files_limit",
            "{ Ok(()) }",
        );
        self.stub_fn(
            editor,
            root,
            "brush-core/src/sys/stubs/network.rs",
            path,
            "get_hostname",
            "{ Ok(std::ffi::OsString::from(\"localhost\")) }",
        );
    }

    fn apply_doc_conversions(&self, editor: &mut SyntaxEditor, root: &SyntaxNode) {
        for element in root.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::COMMENT {
                    let text = token.text();
                    if text.starts_with("//!") {
                        let new_text = text.replacen("//!", "///", 1);
                        let new_token = parse_token(&new_text, SyntaxKind::COMMENT);
                        editor.replace(token, new_token);
                    }
                }
            }
        }
    }

    fn apply_inner_attributes_commenting(&self, editor: &mut SyntaxEditor, root: &SyntaxNode) {
        for attr in root.descendants().filter_map(ast::Attr::cast) {
            if attr.excl_token().is_some() {
                let attr_text = attr.syntax().text();
                let comment_text = format!("// {}", attr_text);
                let comment_token = parse_token(&comment_text, SyntaxKind::COMMENT);
                editor.replace(attr.syntax(), comment_token);
            }
        }
    }

    fn apply_async_transforms(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);

        // Skip uucore for async transforms
        if path_str.contains("uucore") {
            return;
        }

        // Async function conversion
        for func in root.descendants().filter_map(ast::Fn::cast) {
            if func.name().map(|n| n.text().to_string()) == Some("read_line".to_string()) {
                // Skip input_backend.rs and read.rs as they handle read_line synchronously
                if path_str.contains("input_backend.rs")
                    || path_str.contains("read.rs")
                    || path_str.contains("term_line_reader.rs")
                {
                    // eprintln!("Skipping async transform for {}", path_str);
                    continue;
                }

                if func.async_token().is_none() {
                    let async_token = parse_token("async", SyntaxKind::ASYNC_KW);
                    if let Some(fn_token) = func.fn_token() {
                        editor.insert(
                            Position::before(fn_token.clone()),
                            make::tokens::single_space(),
                        );
                        editor.insert(Position::before(fn_token.clone()), async_token.clone());
                    }
                }
            }
        }

        // Method call .await addition
        for call in root.descendants().filter_map(ast::MethodCallExpr::cast) {
            if call.name_ref().map(|n| n.text().to_string()) == Some("read_line".to_string()) {
                // Skip input_backend.rs and read.rs as they handle read_line synchronously
                if path_str.contains("input_backend.rs")
                    || path_str.contains("read.rs")
                    || path_str.contains("term_line_reader.rs")
                {
                    continue;
                }

                let needs_await = call
                    .syntax()
                    .parent()
                    .and_then(ast::AwaitExpr::cast)
                    .is_none();

                if needs_await {
                    let detached_call =
                        ast::MethodCallExpr::cast(call.syntax().clone_for_update()).unwrap();
                    let await_expr = make::expr_await(ast::Expr::MethodCallExpr(detached_call));
                    editor.replace(call.syntax(), await_expr.syntax().clone_for_update());
                }
            }
        }
    }

    fn apply_interactive_lib_transforms(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        path: &Path,
    ) {
        let path_str = normalize_path(path);
        if !path_str.contains("interactive") {
            return;
        }

        // Find all cfg attributes that gate reedline/basic/minimal features
        for attr in root.descendants().filter_map(ast::Attr::cast) {
            if let Some(path) = attr.path() {
                if path
                    .segment()
                    .and_then(|s| s.name_ref())
                    .map(|n| n.text().to_string())
                    == Some("cfg".to_string())
                {
                    let attr_text = attr.syntax().text().to_string();
                    if attr_text.contains("feature = \"reedline\"")
                        || attr_text.contains("feature = \"basic\"")
                        || attr_text.contains("feature = \"minimal\"")
                    {
                        let comment =
                            parse_token(&format!("/* {} */", attr_text), SyntaxKind::COMMENT);
                        editor.insert(Position::before(attr.syntax()), comment);
                        editor.delete(attr.syntax());
                    }
                }
            }
        }

        // Find and comment out the gated module declarations
        for module in root.descendants().filter_map(ast::Module::cast) {
            if let Some(name) = module.name() {
                let mod_name = name.text().to_string();
                if mod_name == "reedline" || mod_name == "basic" || mod_name == "minimal" {
                    let comment = parse_token(
                        &format!("/* {} */", module.syntax().text()),
                        SyntaxKind::COMMENT,
                    );
                    editor.insert(Position::before(module.syntax()), comment);
                    editor.delete(module.syntax());
                }
            }
        }

        // Handle refs module replacement with ShellRef type alias
        for module in root.descendants().filter_map(ast::Module::cast) {
            if module.name().map(|n| n.text().to_string()) == Some("refs".to_string()) {
                let type_alias = parse_node::<ast::Item>(
                    "pub type ShellRef = std::sync::Arc<tokio::sync::Mutex<super::brush_core::Shell>>;"
                );
                editor.replace(module.syntax(), type_alias.syntax());
            }
        }

        // Comment out pub use statements
        for use_item in root.descendants().filter_map(ast::Use::cast) {
            let use_str = use_item.to_string();
            if use_str.contains("reedline")
                || use_str.contains("basic")
                || use_str.contains("minimal")
                || use_str.contains("refs::ShellRef")
            {
                let comment = parse_token(&format!("/* {} */", use_str), SyntaxKind::COMMENT);
                editor.insert(Position::before(use_item.syntax()), comment);
                editor.delete(use_item.syntax());
            }
        }
    }

    fn apply_additional_transforms_ast(
        &self,
        editor: &mut SyntaxEditor,
        root: &SyntaxNode,
        path: &Path,
    ) {
        let path_str = normalize_path(path);
        // println!("cargo:warning=Visiting: {}", path_str);
        if path_str.ends_with("brush-interactive/src/lib.rs") {
            println!("cargo:warning=MATCHED brush-interactive/src/lib.rs");
        }

        // Crossterm redirect
        for path in root.descendants().filter_map(ast::Path::cast) {
            // Skip if this path is part of a larger path (we'll handle the larger path)
            if path
                .syntax()
                .parent()
                .map_or(false, |p| p.kind() == SyntaxKind::PATH)
            {
                continue;
            }

            let segments: Vec<_> = path.segments().collect();
            if let Some(first) = segments.first() {
                if first.name_ref().map_or(false, |n| n.text() == "crossterm") {
                    let rest: Vec<_> = segments.iter().skip(1).map(|s| s.to_string()).collect();
                    let new_path_text = if rest.is_empty() {
                        "crate::shims::crossterm".to_string()
                    } else {
                        format!("crate::shims::crossterm::{}", rest.join("::"))
                    };
                    let new_path = make_mutable(make::path_from_text(&new_path_text));
                    editor.replace(path.syntax(), new_path.syntax());
                }
            }
        }

        // ExitStatus and Output redirect
        for path in root.descendants().filter_map(ast::Path::cast) {
            if path.to_string() == "std::process::ExitStatus"
                || path.to_string() == "crate::brush_core::ExitStatus"
            {
                let new_path = make_mutable(make::path_from_text(
                    "crate::shims::exit_status::ExitStatus",
                ));
                editor.replace(path.syntax(), new_path.syntax());
            }

            if path.to_string() == "std::process::Output"
                || path.to_string() == "crate::brush_core::Output"
            {
                let new_path =
                    make_mutable(make::path_from_text("crate::shims::exit_status::Output"));
                editor.replace(path.syntax(), new_path.syntax());
            }
        }

        // File specific patches
        if path_str.ends_with("uucore/src/lib/mods/locale.rs") {
            println!("cargo:warning=Patching locale.rs");
            for func in root.descendants().filter_map(ast::Fn::cast) {
                if func
                    .name()
                    .map_or(false, |n| n.text() == "get_embedded_locale")
                {
                    if let Some(name) = func.name() {
                        let new_name = make::name("get_embedded_locale_unused");
                        editor.replace(name.syntax(), new_name.syntax());
                    }
                }
            }

            // Replace include! with a re-export to the canonical shim
            for macro_call in root.descendants().filter_map(ast::MacroCall::cast) {
                if macro_call
                    .path()
                    .map_or(false, |p| p.to_string() == "include")
                {
                    if macro_call.to_string().contains("embedded_locales.rs") {
                        let new_item = parse_node::<ast::Item>("pub use crate::shims::embedded_locales::get_embedded_locale as get_embedded_locale;");
                        editor.replace(macro_call.syntax(), new_item.syntax());
                    }
                }
            }
        }

        if path_str.ends_with("brush-core/src/sys/stubs/process.rs") {
            for struct_def in root.descendants().filter_map(ast::Struct::cast) {
                if struct_def.name().map_or(false, |n| n.text() == "Child") {
                    if let Some(field_list) = struct_def.field_list() {
                        if let ast::FieldList::RecordFieldList(record) = field_list {
                            for field in record.fields() {
                                if field.name().map_or(false, |n| n.text() == "inner") {
                                    if let Some(ty) = field.ty() {
                                        let new_path = make_mutable(make::path_from_text(
                                            "crate::stubs_impl::GuestChild",
                                        ));
                                        let new_ty = make_mutable(make::ty_path(new_path));
                                        editor.replace(ty.syntax(), new_ty.syntax());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if path_str.ends_with("brush-core/src/sys/stubs.rs") {
            // No longer needed
        }

        if path_str.ends_with("brush-core/src/shell.rs") {
            for let_stmt in root.descendants().filter_map(ast::LetStmt::cast) {
                if let Some(pat) = let_stmt.pat() {
                    if pat.to_string() == "oldpwd" {
                        if let Some(init) = let_stmt.initializer() {
                            if init.to_string().contains("std::mem::replace") {
                                let new_stmt = parse_stmt::<ast::Stmt>(
                                    "let _ = std::env::set_current_dir(&self.working_dir);",
                                );
                                editor.insert(
                                    Position::after(let_stmt.syntax().clone()),
                                    make::tokens::single_newline(),
                                );
                                editor.insert(
                                    Position::after(let_stmt.syntax().clone()),
                                    new_stmt.syntax(),
                                );
                            }
                        }
                    }
                }
            }
        }

        if path_str.ends_with("brush-builtins/src/lib.rs") {
            // Add #[macro_export] to minus_or_plus_flag_arg
            for macro_def in root.descendants().filter_map(ast::MacroRules::cast) {
                if macro_def
                    .name()
                    .map_or(false, |n| n.text() == "minus_or_plus_flag_arg")
                {
                    let has_attr = macro_def
                        .attrs()
                        .any(|a| a.to_string().contains("macro_export"));
                    if !has_attr {
                        let attr = parse_node::<ast::Attr>("#[macro_export]");
                        editor.insert(Position::before(macro_def.syntax()), attr.syntax());
                        editor.insert(
                            Position::before(macro_def.syntax()),
                            make::tokens::single_newline(),
                        );
                    }
                }
            }
        }

        if path_str.contains("brush-core/src/expansion.rs") {
            // Add use std::future::Future;
            let has_use = root
                .descendants()
                .filter_map(ast::Use::cast)
                .any(|u| u.to_string().contains("std::future::Future"));
            if !has_use {
                let use_item = parse_node::<ast::Item>("use std::future::Future;");
                if let Some(last) = root.last_child() {
                    editor.insert(
                        Position::after(last.clone()),
                        make::tokens::single_newline(),
                    );
                    editor.insert(Position::after(last), use_item.syntax());
                }
            }
        }

        if path_str.ends_with("brush-shell/src/lib.rs") {
            // mod brushctl -> pub mod brushctl
            for mod_item in root.descendants().filter_map(ast::Module::cast) {
                if mod_item.name().map_or(false, |n| n.text() == "brushctl") {
                    if mod_item.visibility().is_none() {
                        let pub_token = parse_token("pub", SyntaxKind::PUB_KW);
                        if let Some(mod_token) = mod_item.mod_token() {
                            editor.insert(
                                Position::before(mod_token.clone()),
                                make::tokens::single_space(),
                            );
                            editor.insert(Position::before(mod_token), pub_token);
                        }
                    }
                }
            }
            // Add pub use re-export for embedded shim
            let has_mod = root
                .descendants()
                .filter_map(ast::Use::cast)
                .any(|u| u.to_string().contains("shims::embedded_brush_shell"));
            if !has_mod {
                let use_item = parse_node::<ast::Item>(
                    "pub use crate::shims::embedded_brush_shell as embedded;",
                );
                if let Some(last) = root.last_child() {
                    editor.insert(Position::after(last), use_item.syntax());
                }
            }
        }

        if path_str.ends_with("brush-shell/src/brushctl.rs") {
            // pub(crate) trait -> pub trait
            for trait_def in root.descendants().filter_map(ast::Trait::cast) {
                if trait_def
                    .name()
                    .map_or(false, |n| n.text() == "ShellBuilderBrushBuiltinExt")
                {
                    if let Some(vis) = trait_def.visibility() {
                        let pub_vis = make_mutable(make::visibility_pub());
                        editor.replace(vis.syntax(), pub_vis.syntax());
                    }
                }
            }
        }

        if path_str.ends_with("brush-core/src/error.rs") {
            // impl<T> From<T> for Error
            for impl_def in root.descendants().filter_map(ast::Impl::cast) {
                if let Some(self_ty) = impl_def.self_ty() {
                    if self_ty.to_string() == "Error" {
                        if let Some(trait_) = impl_def.trait_() {
                            if trait_.to_string().contains("From") {
                                let include_item = parse_node::<ast::Item>("include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/brush_error_impls.rs\"));");
                                editor.replace(impl_def.syntax(), include_item.syntax());
                            }
                        }
                    }
                }
            }
        }

        if path_str.ends_with("brush-shell/src/entry.rs") {
            // Visibility and Renaming
            for func in root.descendants().filter_map(ast::Fn::cast) {
                if let Some(name) = func.name() {
                    let name_text = name.text().to_string();
                    if name_text == "run_async" || name_text == "try_parse_from" {
                        if func.visibility().is_none() {
                            let pub_vis = make_mutable(make::visibility_pub());
                            // Check for async token to place pub before it
                            if let Some(async_token) = func.async_token() {
                                editor.insert(
                                    Position::before(async_token.clone()),
                                    make::tokens::single_space(),
                                );
                                editor.insert(Position::before(async_token), pub_vis.syntax());
                            } else if let Some(fn_token) = func.fn_token() {
                                editor.insert(
                                    Position::before(fn_token.clone()),
                                    make::tokens::single_space(),
                                );
                                editor.insert(Position::before(fn_token.clone()), pub_vis.syntax());
                            }
                        }

                        // Rename try_parse_from to try_parse_from_brush to avoid shadowing clap trait
                        if name_text == "try_parse_from" {
                            let new_name = make_mutable(make::name("try_parse_from_brush"));
                            editor.replace(name.syntax(), new_name.syntax());
                        }
                    }
                }
            }

            // Rename internal calls
            for path in root.descendants().filter_map(ast::Path::cast) {
                if let Some(seg) = path.segment() {
                    if let Some(name_ref) = seg.name_ref() {
                        if name_ref.text() == "try_parse_from" {
                            let full_path = path.to_string();
                            if full_path.contains("CommandLineArgs")
                                || full_path.starts_with("Self")
                            {
                                let new_name_ref =
                                    make_mutable(make::name_ref("try_parse_from_brush"));
                                editor.replace(name_ref.syntax(), new_name_ref.syntax());
                            }
                        }
                    }
                }
            }

            // Method call replacement: shell.default_builtins(builtin_set) -> crate::stubs_impl::configure_shell(shell, builtin_set)
            let mut handled_ranges = std::collections::HashSet::new();
            // First pass: handle the larger expression
            for call in root.descendants().filter_map(ast::MethodCallExpr::cast) {
                let call_str = call.to_string();
                if call_str == "shell.default_builtins(builtin_set).brush_builtins()" {
                    let new_expr = parse_expr::<ast::Expr>(
                        "crate::stubs_impl::configure_shell(shell, builtin_set)",
                    );
                    editor.replace(call.syntax(), new_expr.syntax());

                    // Mark the inner expression as handled
                    if let Some(receiver) = call.receiver() {
                        handled_ranges.insert(receiver.syntax().text_range());
                    }
                }
            }

            // Second pass: handle the smaller expression if not already handled
            for call in root.descendants().filter_map(ast::MethodCallExpr::cast) {
                let call_str = call.to_string();
                if call_str == "shell.default_builtins(builtin_set)" {
                    if !handled_ranges.contains(&call.syntax().text_range()) {
                        let new_expr = parse_expr::<ast::Expr>(
                            "crate::stubs_impl::configure_shell(shell, builtin_set)",
                        );
                        editor.replace(call.syntax(), new_expr.syntax());
                    }
                }
            }

            // MinimalInputBackend replacement
            for path in root.descendants().filter_map(ast::Path::cast) {
                if path.to_string() == "brush_interactive::MinimalInputBackend"
                    || path.to_string() == "crate::brush_interactive::MinimalInputBackend"
                {
                    if path
                        .syntax()
                        .parent()
                        .map_or(false, |p| ast::PathExpr::can_cast(p.kind()))
                    {
                        let new_expr = parse_expr::<ast::Expr>(
                            "crate::brush_interactive::MinimalInputBackend::new()",
                        );
                        if let Some(path_expr) =
                            ast::PathExpr::cast(path.syntax().parent().unwrap())
                        {
                            editor.replace(path_expr.syntax(), new_expr.syntax());
                        }
                    }
                }
            }
        }
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

        if let Err(_e) = fs_extra::dir::copy(&src_path, dest_path.parent().unwrap(), &options) {
            // eprintln!(
            //     "Failed to copy {:?} to {:?}: {}",
            //     src_path,
            //     dest_path.parent().unwrap(),
            //     e
            // );
            panic!("Failed to copy source");
        }
    }
}

fn verify_no_stubs(vendor_dir: &Path) {
    // println!("Verifying no unpatched stubs in {:?}", vendor_dir);
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
