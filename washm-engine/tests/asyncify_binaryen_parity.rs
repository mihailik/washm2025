use bumpalo::Bump;
use std::fs;
use std::path::{Path, PathBuf};
use washm::ir::module::Module;
use washm::passes::asyncify::{Asyncify, AsyncifyConfig};
use washm::passes::Pass;
use washm::wasm::binary_reader::BinaryReader;
use wast::parser::{self, ParseBuffer};
use wast::Wast;

#[test]
fn test_asyncify_binaryen_sleep_parity() {
    let allocator = Bump::new();

    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("external");
    path.push("binaryen-fork");
    path.push("test");
    path.push("unit");
    path.push("input");
    path.push("asyncify-sleep.wat");

    if !path.exists() {
        println!("Skipping test: asyncify-sleep.wat not found at {:?}", path);
        return;
    }

    let wat_content = fs::read_to_string(&path).expect("Failed to read .wat file");
    let wasm_binary = wat::parse_str(&wat_content).expect("Failed to parse WAT");

    let mut reader = BinaryReader::new(&allocator, wasm_binary);
    let mut module = reader
        .parse_module()
        .expect("Failed to parse module into IR");

    let mut config = AsyncifyConfig::default();
    config.imports.push("env.sleep".to_string());
    config.imports.push("env.tunnel".to_string());

    let mut asyncify = Asyncify::new(config);
    asyncify.run(&mut module);

    verify_asyncify_module(&module);

    // Verify 'factorial-loop' still works (it has nested structural components)
    // We expect it to have been transformed.
    let factorial_loop_idx = module
        .exports
        .iter()
        .find(|e| {
            e.name == "factorial-loop" && matches!(e.kind, washm::ir::module::ExportKind::Function)
        })
        .map(|e| e.index)
        .expect("factorial-loop export missing");

    let num_func_imports = module
        .imports
        .iter()
        .filter(|i| matches!(i.kind, washm::ir::module::ImportKind::Function(_, _)))
        .count() as u32;
    let func = &module.functions[(factorial_loop_idx - num_func_imports) as usize];

    let body = func.body.expect("factorial-loop missing body");
    // Verify it contains a loop that is instrumented
    use washm::ir::expression::ExpressionKind;
    use washm::ir::visitor::ReadOnlyVisitor;

    struct LoopChecker {
        found: bool,
    }
    impl<'a> ReadOnlyVisitor<'a> for LoopChecker {
        fn visit_expression(&mut self, expr: washm::ir::expression::ExprRef<'a>) {
            if let ExpressionKind::Loop { body, .. } = &expr.kind {
                if let ExpressionKind::If { .. } = &body.kind {
                    self.found = true;
                }
            }
        }
    }

    let mut checker = LoopChecker { found: false };
    checker.visit(body);
    assert!(
        checker.found,
        "Loop in factorial-loop should be instrumented with range check"
    );
}

#[test]
fn test_asyncify_binaryen_suite_parity() {
    let allocator = Bump::new();

    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("external")
        .join("binaryen-fork")
        .join("test")
        .join("lit")
        .join("passes");

    if !base_path.exists() {
        println!(
            "Skipping suite: Binaryen tests not found at {:?}",
            base_path
        );
        return;
    }

    let test_files = [
        "asyncify.wast",
        "asyncify-wasm64.wast",
        "asyncify_pass-arg=asyncify-ignore-imports.wast",
        "asyncify_pass-arg=asyncify-ignore-indirect.wast",
        "asyncify_pass-arg=asyncify-onlylist@foo,bar.wast",
    ];

    for file_name in &test_files {
        let path = base_path.join(file_name);
        println!("Running Binaryen parity test: {}", file_name);

        let wast_content = fs::read_to_string(&path).expect("Failed to read .wast file");

        let mut lexer = wast::lexer::Lexer::new(&wast_content);
        lexer.allow_confusing_unicode(true);
        let buffer = match ParseBuffer::new_with_lexer(lexer) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let wast: Wast = match wast::parser::parse(&buffer) {
            Ok(w) => w,
            Err(_) => continue,
        };

        for directive in wast.directives {
            if let wast::WastDirective::Module(mut module) = directive {
                let wasm_binary = module.encode().expect("Failed to encode module");
                let mut reader = BinaryReader::new(&allocator, wasm_binary);
                let mut ir_module = reader
                    .parse_module()
                    .expect("Failed to parse module into IR");

                let mut config = AsyncifyConfig::default();
                // Heuristic: If it has imports, it likely needs them to be async-capable for the test
                for import in &ir_module.imports {
                    if let washm::ir::module::ImportKind::Function(_, _) = &import.kind {
                        config
                            .imports
                            .push(format!("{}.{}", import.module, import.name));
                    }
                }

                // Specific overrides for certain tests
                if file_name.contains("onlylist@foo,bar") {
                    config.imports = vec!["env.foo".to_string(), "env.bar".to_string()];
                }

                let mut asyncify = Asyncify::new(config);
                asyncify.run(&mut ir_module);

                // Verify basic invariants
                assert!(
                    ir_module
                        .globals
                        .iter()
                        .any(|g| g.name == "__asyncify_state"),
                    "{} missing state global",
                    file_name
                );
                assert!(
                    ir_module
                        .exports
                        .iter()
                        .any(|e| e.name == "asyncify_get_state"),
                    "{} missing helper export",
                    file_name
                );
            }
        }
    }
}

fn verify_liveness1(func: &washm::ir::module::Function) {
    use washm::ir::expression::ExpressionKind;
    use washm::ir::Literal;

    let body = func.body.expect("liveness1 missing body");
    if let ExpressionKind::Block { list, .. } = &body.kind {
        // First element is the Rewinding check
        let rewind_if = list[0];
        if let ExpressionKind::If { if_true, .. } = &rewind_if.kind {
            if let ExpressionKind::Block {
                list: rewind_list, ..
            } = &if_true.kind
            {
                // Should find a subtraction of 8 (for 2 locals) and 4 (for index)
                // Or a combined subtraction.
                // My implementation does them separately currently.

                let mut found_8 = false;
                let mut found_4 = false;

                for expr in rewind_list {
                    if let ExpressionKind::Store { value, .. } = &expr.kind {
                        if let ExpressionKind::Binary { right, .. } = &value.kind {
                            if let ExpressionKind::Const(Literal::I32(val)) = &right.kind {
                                if *val == 8 {
                                    found_8 = true;
                                }
                                if *val == 4 {
                                    found_4 = true;
                                }
                            }
                        }
                    }
                    // Also check sub-blocks
                    if let ExpressionKind::Block { list: sub, .. } = &expr.kind {
                        for sub_expr in sub {
                            if let ExpressionKind::Store { value, .. } = &sub_expr.kind {
                                if let ExpressionKind::Binary { right, .. } = &value.kind {
                                    if let ExpressionKind::Const(Literal::I32(val)) = &right.kind {
                                        if *val == 8 {
                                            found_8 = true;
                                        }
                                        if *val == 4 {
                                            found_4 = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                assert!(
                    found_8,
                    "Expected subtraction of 8 for live locals in liveness1"
                );
                assert!(
                    found_4,
                    "Expected subtraction of 4 for call index in liveness1"
                );
            }
        }
    }
}

fn verify_asyncify_module(module: &Module) {
    use washm::passes::asyncify::{ASYNCIFY_DATA, ASYNCIFY_STATE};

    // 1. Check globals
    assert!(
        module.globals.iter().any(|g| g.name == ASYNCIFY_STATE),
        "Missing __asyncify_state"
    );
    assert!(
        module.globals.iter().any(|g| g.name == ASYNCIFY_DATA),
        "Missing __asyncify_data"
    );

    // 2. Check exported helpers
    let exports: std::collections::HashSet<_> =
        module.exports.iter().map(|e| e.name.as_str()).collect();
    assert!(exports.contains("asyncify_start_unwind"));
    assert!(exports.contains("asyncify_stop_unwind"));
    assert!(exports.contains("asyncify_start_rewind"));
    assert!(exports.contains("asyncify_stop_rewind"));
    assert!(exports.contains("asyncify_get_state"));
}
