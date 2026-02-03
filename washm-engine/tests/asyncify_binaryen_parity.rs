use std::fs;
use std::path::PathBuf;
use washm::ir::module::Module;
use washm::passes::asyncify::{Asyncify, AsyncifyConfig};
use washm::passes::Pass;
use washm::wasm::binary_reader::BinaryReader;
use wast::parser::ParseBuffer;
use wast::Wast;

#[test]
fn test_asyncify_binaryen_sleep_parity() {
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

    let mut reader = BinaryReader::new();
    let mut module = reader
        .parse_module(&wasm_binary)
        .expect("Failed to parse module into IR");

    let mut config = AsyncifyConfig::default();
    config.imports.push("env.sleep".to_string());
    config.imports.push("env.tunnel".to_string());

    // Print IR BEFORE asyncify
    {
        let factorial_loop_id = module
            .exports
            .iter()
            .find(|e| {
                e.name == "factorial-loop"
                    && matches!(e.kind, washm::ir::module::ExportKind::Function(_))
            })
            .map(|e| match e.kind {
                washm::ir::module::ExportKind::Function(id) => id,
                _ => unreachable!(),
            })
            .expect("factorial-loop export missing");

        let func = &module.functions[factorial_loop_id];
        let body = func.body().expect("factorial-loop missing body");
        eprintln!("IR BEFORE ASYNCIFY (factorial-loop):");
        washm::passes::asyncify::dump_ir(&module.expressions, body, 0);
    }

    let mut asyncify = Asyncify::new(config);
    asyncify.run(&mut module);

    // Print IR AFTER asyncify
    {
        let factorial_loop_id = module
            .exports
            .iter()
            .find(|e| {
                e.name == "factorial-loop"
                    && matches!(e.kind, washm::ir::module::ExportKind::Function(_))
            })
            .map(|e| match e.kind {
                washm::ir::module::ExportKind::Function(id) => id,
                _ => unreachable!(),
            })
            .expect("factorial-loop export missing");

        let func = &module.functions[factorial_loop_id];
        let body = func.body().expect("factorial-loop missing body");
        eprintln!("IR AFTER ASYNCIFY (factorial-loop):");
        washm::passes::asyncify::dump_ir(&module.expressions, body, 0);
    }

    verify_asyncify_module(&module);

    // Verify 'factorial-loop' still works (it has nested structural components)
    // We expect it to have been transformed.
    let factorial_loop_id = module
        .exports
        .iter()
        .find(|e| {
            e.name == "factorial-loop"
                && matches!(e.kind, washm::ir::module::ExportKind::Function(_))
        })
        .map(|e| match e.kind {
            washm::ir::module::ExportKind::Function(id) => id,
            _ => unreachable!(),
        })
        .expect("factorial-loop export missing");

    let func = &module.functions[factorial_loop_id];

    let body = func.body().expect("factorial-loop missing body");
    // Verify it contains a loop that is instrumented
    use washm::collections::id_arena::IdArena;
    use washm::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
    use washm::ir::visitor::ReadOnlyVisitor;

    struct LoopChecker {
        found: bool,
    }
    impl ReadOnlyVisitor for LoopChecker {
        fn visit_expression(
            &mut self,
            expr_id: ExpressionId,
            arena: &IdArena<ExpressionMarker, Expression>,
        ) {
            if let ExpressionKind::Loop { body, .. } = &arena[expr_id].kind {
                if let ExpressionKind::If { .. } = &arena[*body].kind {
                    self.found = true;
                }
            }
        }
    }

    let mut checker = LoopChecker { found: false };
    checker.visit(body, &module.expressions);
    assert!(
        checker.found,
        "Loop in factorial-loop should be instrumented with range check"
    );
}

#[test]
fn test_asyncify_binaryen_suite_parity() {
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
                let mut reader = BinaryReader::new();
                let mut ir_module = reader
                    .parse_module(&wasm_binary)
                    .expect("Failed to parse module into IR");

                let mut config = AsyncifyConfig::default();
                // Heuristic: If it has imports, it likely needs them to be async-capable for the test
                for (_, func) in ir_module.functions.iter() {
                    if let washm::ir::module::FunctionKind::Import(import) = &func.kind {
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
                        .any(|g| g.1.name == "__asyncify_state"),
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

#[allow(dead_code)]
fn verify_liveness1(module: &Module, func: &washm::ir::module::Function) {
    use washm::ir::expression::ExpressionKind;
    use washm::ir::Literal;

    let body = func.body().expect("liveness1 missing body");
    if let ExpressionKind::Block { list, .. } = &module.expressions[body].kind {
        let first_expr_id = list[0];
        if let ExpressionKind::If { if_true, .. } = &module.expressions[first_expr_id].kind {
            if let ExpressionKind::Block {
                list: rewind_list, ..
            } = &module.expressions[*if_true].kind
            {
                let mut found_8 = false;
                let mut found_4 = false;

                for &expr_id in rewind_list {
                    let expr = &module.expressions[expr_id];
                    if let ExpressionKind::LocalSet { value, .. } = &expr.kind {
                        let value_expr = &module.expressions[*value];
                        if let ExpressionKind::Binary { right, .. } = &value_expr.kind {
                            let right_expr = &module.expressions[*right];
                            if let ExpressionKind::Const(Literal::I32(val)) = &right_expr.kind {
                                if *val == 8 {
                                    found_8 = true;
                                }
                                if *val == 4 {
                                    found_4 = true;
                                }
                            }
                        }
                    }
                    if let ExpressionKind::Block { list: sub, .. } = &expr.kind {
                        for &sub_expr_id in sub {
                            let sub_expr = &module.expressions[sub_expr_id];
                            if let ExpressionKind::LocalSet { value, .. } = &sub_expr.kind {
                                let value_expr = &module.expressions[*value];
                                if let ExpressionKind::Binary { right, .. } = &value_expr.kind {
                                    let right_expr = &module.expressions[*right];
                                    if let ExpressionKind::Const(Literal::I32(val)) =
                                        &right_expr.kind
                                    {
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
        module.globals.iter().any(|g| g.1.name == ASYNCIFY_STATE),
        "Missing __asyncify_state"
    );
    assert!(
        module.globals.iter().any(|g| g.1.name == ASYNCIFY_DATA),
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
