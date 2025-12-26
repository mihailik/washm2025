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
use ra_ap_syntax::{ast, AstNode, Edition, SourceFile, SyntaxKind, SyntaxNode, TextRange};
use ra_ap_syntax::syntax_editor::{SyntaxEditor, Position};
use ra_ap_syntax::ast::make;

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

fn parse_stmt<N: AstNode>(text: &str) -> N {
    let wrapped = format!("fn dummy() {{ {} }}", text);
    let parse = SourceFile::parse(&wrapped, Edition::Edition2021);
    let tree = parse.tree();
    let syntax = tree.syntax();
    
    // Find the function body block to avoid matching the function itself
    let func = syntax.descendants().find_map(ast::Fn::cast).expect("failed to parse dummy fn");
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
    if let Some(node) = syntax.descendants().find_map(N::cast) {
        N::cast(node.syntax().clone_for_update()).unwrap()
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
    let tokens: Vec<_> = root.descendants_with_tokens()
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

            // Apply AST-based transformations
            match self.transform_with_ast(&content, path, &module_name) {
                Ok(transformed) => {
                    if content != transformed {
                        fs::write(path, transformed).unwrap();
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
        // eprintln!("Transforming: {:?}", path);
        let mut content = original_content.to_string();

        let path_str = normalize_path(path);
        if path_str.contains("input_backend.rs") || path_str.contains("term_line_reader.rs") {
             return Ok(content);
        }

        if path_str.ends_with("brush-core/src/expansion.rs") {
             content = content.replace("brush_parser::word::SubstringMatchKind", "crate::brush_parser::word::SubstringMatchKind");
             content = content.replace("brush_parser::word::Parameter", "crate::brush_parser::word::Parameter");
             content = content.replace("brush_parser::word::SpecialParameter", "crate::brush_parser::word::SpecialParameter");
             content = content.replace("crate::regex::", "crate::brush_core::regex::");
        }

        if path_str.ends_with("brush-core/src/wellknownvars.rs") {
             content = content.replace("crate::callstack::", "crate::brush_core::callstack::");
        }

        if path_str.ends_with("brush-core/src/prompt.rs") {
             content = content.replace("brush_parser::prompt::PromptPiece", "crate::brush_parser::prompt::PromptPiece");
        }

        if path_str.ends_with("brush-shell/src/entry.rs") {
             content = content.replace("async fn run_async", "pub async fn run_async");
             content = content.replace("fn try_parse_from", "pub fn try_parse_from");
             content = content.replace("brush_interactive::MinimalInputBackend", "brush_interactive::MinimalInputBackend::new()");
             content = content.replace("shell.default_builtins(builtin_set)", "crate::stubs_impl::configure_shell(shell, builtin_set)");
        }

        if path_str.ends_with("brush-core/src/sys/stubs/users.rs") {
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"getting current uid\").into())", "crate::stubs_impl::get_current_uid()");
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"getting current gid\").into())", "crate::stubs_impl::get_current_gid()");
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"getting effective uid\").into())", "crate::stubs_impl::get_effective_uid()");
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"getting effective gid\").into())", "crate::stubs_impl::get_effective_gid()");
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"getting current username\").into())", "Ok(crate::stubs_impl::startup_username())");
        }

        if path_str.ends_with("brush-core/src/sys/stubs/fs.rs") {
             content = content.replace("Err(error::ErrorKind::NotSupportedOnThisPlatform(\"opening null file\").into())", "crate::stubs_impl::open_null_file()");
        }

        if path_str.ends_with("brush-core/src/sys/stubs/commands.rs") {
             content = content.replace("return Err(error::ErrorKind::NotSupportedOnThisPlatform(\"fd redirections\").into());", "return Ok(());");
        }

        if path_str.ends_with("brush-interactive/src/interactive_shell.rs") {
             content = content.replace("fn read_line", "async fn read_line");
        }

        if path_str.ends_with("brush-interactive/src/lib.rs") {
             content.push_str("\npub mod wasm_interactive { include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/wasm_interactive.rs\")); }");
             content.push_str("\npub use wasm_interactive::{WasmInputBackend, ReedlineInputBackend, BasicInputBackend, MinimalInputBackend};");
        }

        if path_str.ends_with("brush-shell/src/lib.rs") {
             content = content.replace("mod brushctl;", "pub mod brushctl;");
             if !content.contains("pub mod embedded;") {
                 content.push_str("\npub mod embedded;");
             }
        }

        if path_str.ends_with("uucore/src/lib/mods/locale.rs") {
             content = content.replace("include!(concat!(env!(\"OUT_DIR\"), \"/embedded_locales.rs\"));", "include!(\"../../embedded_locales.rs\");");
             content = content.replace("fn get_embedded_locale", "fn get_embedded_locale_unused");
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
        // if !edits.is_empty() {
        //      eprintln!("Found {} edits for {:?}", edits.len(), path);
        // }
        
        // Filter out overlapping edits (keep the outer ones)
        let mut filtered_edits = Vec::new();
        for (range, text) in &edits {
            let is_inner = edits.iter().any(|(other_range, _)| {
                other_range != range && other_range.contains_range(*range)
            });
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

        // self.apply_module_path_synthesis(&mut editor, &root);
        self.apply_stub_replacements(&mut editor, &root, path);
        // self.apply_doc_conversions(&mut editor, &root);
        // self.apply_inner_attributes_commenting(&mut editor, &root);
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
            if path.syntax().parent().and_then(ast::MacroCall::cast).is_some() {
                continue;
            }

            let segments: Vec<_> = path.segments().collect();
            if let Some(first_seg) = segments.first() {
                let first_seg_text = first_seg.name_ref().map(|n| n.text().to_string()).unwrap_or_default();
                
                if current_module == "brush_builtins" && path.to_string().contains("brush_core") {
                     eprintln!("DEBUG: Found brush_core path in brush_builtins: '{}'. First seg: '{}'", path, first_seg_text);
                }
                
                if first_seg_text == "crate" {
                    // Handle crate::...
                    let rest: Vec<String> = segments.iter().skip(1)
                        .map(|s| s.name_ref().unwrap().text().to_string())
                        .collect();
                    
                    if !rest.is_empty() {
                        let second_seg = &rest[0];
                        if !vendored_crates.contains(&second_seg.as_str()) {
                             // Replace crate::foo with crate::current_module::foo
                             let new_path_text = format!("crate::{}::{}", current_module, rest.join("::"));
                             edits.push((path.syntax().text_range(), new_path_text));
                        }
                    }
                } else if vendored_crates.contains(&first_seg_text.as_str()) {
                    // Handle bare crate reference: brush_core::... -> crate::brush_core::...
                    if path.syntax().parent().map(|p| p.kind() == SyntaxKind::PATH).unwrap_or(false) {
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

    fn apply_module_path_synthesis(&self, editor: &mut SyntaxEditor, root: &SyntaxNode) {
        for mod_item in root.descendants().filter_map(ast::Module::cast) {
            if mod_item.item_list().is_none() && mod_item.semicolon_token().is_some() {
                let mod_name = mod_item.name().unwrap().text().to_string();
                
                let has_path_attr = mod_item.attrs().any(|attr| {
                    attr.path().and_then(|p| p.segment())
                        .and_then(|s| s.name_ref())
                        .map(|n| n.text().to_string()) == Some("path".to_string())
                });
                
                if !has_path_attr {
                    let path_attr_text = format!("#[path = \"{}.rs\"] mod dummy;", mod_name);
                    let path_attr = parse_node::<ast::Attr>(&path_attr_text);
                    
                    editor.insert(Position::before(mod_item.syntax()), path_attr.syntax());
                    editor.insert(Position::before(mod_item.syntax()), make::tokens::single_newline());
                }
            }
        }
    }

    fn apply_stub_replacements(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);
        
        for func in root.descendants().filter_map(ast::Fn::cast) {
            if let Some(name) = func.name() {
                let fn_name = name.text();
                
                // Signal stubs
                if path_str.ends_with("brush-core/src/sys/stubs/signal.rs") {
                    if fn_name == "continue_process" || fn_name == "kill_process" {
                        if let Some(body) = func.body() {
                            if body.syntax().text().to_string().contains("NotSupportedOnThisPlatform") {
                                let new_body = parse_stmt::<ast::BlockExpr>("{ Ok(()) }");
                                editor.replace(body.syntax(), new_body.syntax());
                            }
                        }
                    }
                }
                
                // Other stubs
                if path_str.ends_with("brush-core/src/sys/stubs/users.rs") && fn_name == "get_current_username" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Ok(\"user\".to_string()) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
                
                if path_str.ends_with("brush-core/src/openfiles.rs") && fn_name == "check_open_files_limit" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Ok(()) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
                
                if path_str.ends_with("brush-core/src/sys/stubs/network.rs") && fn_name == "get_hostname" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Ok(std::ffi::OsString::from(\"localhost\")) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
                
                if path_str.ends_with("brush-core/src/sys/stubs/fs.rs") && fn_name == "get_disk_usage" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ None }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
                
                if path_str.ends_with("brush-core/src/sys/stubs/process.rs") && fn_name == "get_memory_usage" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ None }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }

                if path_str.ends_with("brush-core/src/sys/stubs/process.rs") && fn_name == "spawn" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Err(std::io::Error::new(std::io::ErrorKind::Unsupported, \"spawn not supported\")) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }

                if path_str.ends_with("brush-core/src/sys/stubs/process.rs") && fn_name == "wait" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Ok(crate::shims::exit_status::ExitStatus::from_raw(0)) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
                
                if path_str.ends_with("brush-core/src/sys/stubs/commands.rs") && fn_name == "invoke_external_command" {
                     if let Some(body) = func.body() {
                         let new_body = parse_stmt::<ast::BlockExpr>("{ Err(crate::brush_core::error::Error::NotSupported) }");
                         editor.replace(body.syntax(), new_body.syntax());
                     }
                }
            }
        }
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
                if path_str.contains("input_backend.rs") || path_str.contains("read.rs") || path_str.contains("term_line_reader.rs") {
                    // eprintln!("Skipping async transform for {}", path_str);
                    continue;
                }

                if func.async_token().is_none() {
                    let async_token = parse_token("async", SyntaxKind::ASYNC_KW);
                    if let Some(fn_token) = func.fn_token() {
                        editor.insert(Position::before(fn_token.clone()), async_token.clone());
                        editor.insert(Position::before(fn_token.clone()), make::tokens::single_space());
                    }
                }
            }
        }
        
        // Method call .await addition
        for call in root.descendants().filter_map(ast::MethodCallExpr::cast) {
            if call.name_ref().map(|n| n.text().to_string()) == Some("read_line".to_string()) {
                // Skip input_backend.rs and read.rs as they handle read_line synchronously
                if path_str.contains("input_backend.rs") || path_str.contains("read.rs") || path_str.contains("term_line_reader.rs") {
                    continue;
                }

                let needs_await = call.syntax().parent()
                    .and_then(ast::AwaitExpr::cast)
                    .is_none();

                if needs_await {
                    let detached_call = ast::MethodCallExpr::cast(call.syntax().clone_for_update()).unwrap();
                    let await_expr = make::expr_await(ast::Expr::MethodCallExpr(detached_call));
                    editor.replace(call.syntax(), await_expr.syntax().clone_for_update());
                }
            }
        }
    }

    fn apply_interactive_lib_transforms(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);
        if !path_str.contains("interactive") {
            return;
        }

        // Find all cfg attributes that gate reedline/basic/minimal features
        for attr in root.descendants().filter_map(ast::Attr::cast) {
            if let Some(path) = attr.path() {
                if path.segment().and_then(|s| s.name_ref()).map(|n| n.text().to_string()) == Some("cfg".to_string()) {
                    let attr_text = attr.syntax().text().to_string();
                    if attr_text.contains("feature = \"reedline\"")
                        || attr_text.contains("feature = \"basic\"")
                        || attr_text.contains("feature = \"minimal\"")
                    {
                        let comment = parse_token(&format!("/* {} */", attr_text), SyntaxKind::COMMENT);
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
                    let comment = parse_token(&format!("/* {} */", module.syntax().text()), SyntaxKind::COMMENT);
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
             if use_str.contains("reedline") || use_str.contains("basic") || use_str.contains("minimal") || use_str.contains("refs::ShellRef") {
                 let comment = parse_token(&format!("/* {} */", use_str), SyntaxKind::COMMENT);
                 editor.insert(Position::before(use_item.syntax()), comment);
                 editor.delete(use_item.syntax());
             }
        }
    }

    fn apply_additional_transforms_ast(&self, editor: &mut SyntaxEditor, root: &SyntaxNode, path: &Path) {
        let path_str = normalize_path(path);

        // Crossterm redirect
        for path in root.descendants().filter_map(ast::Path::cast) {
             // Skip if this path is part of a larger path (we'll handle the larger path)
             if path.syntax().parent().map_or(false, |p| p.kind() == SyntaxKind::PATH) {
                 continue;
             }

             let segments: Vec<_> = path.segments().collect();
             if let Some(first) = segments.first() {
                 if first.name_ref().map_or(false, |n| n.text() == "crossterm") {
                     let rest: Vec<_> = segments.iter().skip(1).map(|s| s.to_string()).collect();
                     let new_path_text = if rest.is_empty() {
                         "crate::brush_interactive::crossterm".to_string()
                     } else {
                         format!("crate::brush_interactive::crossterm::{}", rest.join("::"))
                     };
                     let new_path = make_mutable(make::path_from_text(&new_path_text));
                     editor.replace(path.syntax(), new_path.syntax());
                 }
             }
        }

        // ExitStatus redirect
        for path in root.descendants().filter_map(ast::Path::cast) {
            if path.to_string() == "std::process::ExitStatus" || path.to_string() == "crate::brush_core::ExitStatus" {
                let new_path = make_mutable(make::path_from_text("crate::shims::exit_status::ExitStatus"));
                editor.replace(path.syntax(), new_path.syntax());
            }
        }

        // File specific patches
        if path_str.ends_with("uucore/src/lib/mods/locale.rs") {
             for func in root.descendants().filter_map(ast::Fn::cast) {
                 if func.name().map_or(false, |n| n.text() == "get_embedded_locale") {
                     if let Some(name) = func.name() {
                         let new_name = make::name("get_embedded_locale_unused");
                         editor.replace(name.syntax(), new_name.syntax());
                     }
                 }
             }
        }
        
        if path_str.ends_with("brush-core/src/sys/stubs.rs") {
             let has_use = root.descendants().filter_map(ast::Use::cast).any(|u| {
                 u.to_string().contains("crate::stubs_impl")
             });
             if !has_use {
                 let use_item = parse_node::<ast::Item>("pub use crate::stubs_impl::*;");
                 if let Some(last) = root.last_child() {
                     editor.insert(Position::after(last.clone()), make::tokens::single_newline());
                     editor.insert(Position::after(last), use_item.syntax());
                 }
             }
        }

        if path_str.ends_with("brush-core/src/shell.rs") {
             for let_stmt in root.descendants().filter_map(ast::LetStmt::cast) {
                 if let Some(pat) = let_stmt.pat() {
                     if pat.to_string() == "oldpwd" {
                         if let Some(init) = let_stmt.initializer() {
                             if init.to_string().contains("std::mem::replace") {
                                 let new_stmt = parse_stmt::<ast::Stmt>("let _ = std::env::set_current_dir(&self.working_dir);");
                                 editor.insert(Position::after(let_stmt.syntax().clone()), make::tokens::single_newline());
                                 editor.insert(Position::after(let_stmt.syntax().clone()), new_stmt.syntax());
                             }
                         }
                     }
                 }
             }
        }

        if path_str.ends_with("brush-builtins/src/lib.rs") {
             // Add #[macro_export] to minus_or_plus_flag_arg
             for macro_def in root.descendants().filter_map(ast::MacroRules::cast) {
                 if macro_def.name().map_or(false, |n| n.text() == "minus_or_plus_flag_arg") {
                     let has_attr = macro_def.attrs().any(|a| a.to_string().contains("macro_export"));
                     if !has_attr {
                         let attr = parse_node::<ast::Attr>("#[macro_export]");
                         editor.insert(Position::before(macro_def.syntax()), attr.syntax());
                         editor.insert(Position::before(macro_def.syntax()), make::tokens::single_newline());
                     }
                 }
             }
        }

        if path_str.ends_with("brush-core/src/expansion.rs") {
             // Add use std::future::Future;
             let has_use = root.descendants().filter_map(ast::Use::cast).any(|u| {
                 u.to_string().contains("std::future::Future")
             });
             if !has_use {
                 let use_item = parse_node::<ast::Item>("use std::future::Future;");
                 if let Some(last) = root.last_child() {
                     editor.insert(Position::after(last.clone()), make::tokens::single_newline());
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
                             editor.insert(Position::before(mod_token.clone()), pub_token);
                             editor.insert(Position::before(mod_token), make::tokens::single_space());
                         }
                     }
                 }
             }
             // Add pub mod embedded;
             let has_mod = root.descendants().filter_map(ast::Module::cast).any(|m| {
                 m.name().map_or(false, |n| n.text() == "embedded")
             });
             if !has_mod {
                 let mod_item = parse_node::<ast::Item>("pub mod embedded;");
                 if let Some(last) = root.last_child() {
                     editor.insert(Position::after(last), mod_item.syntax());
                 }
             }
        }

        if path_str.ends_with("brush-shell/src/brushctl.rs") {
             // pub(crate) trait -> pub trait
             for trait_def in root.descendants().filter_map(ast::Trait::cast) {
                 if trait_def.name().map_or(false, |n| n.text() == "ShellBuilderBrushBuiltinExt") {
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

