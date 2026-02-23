mod common;

#[test]
fn test_match_reconstruction() {
    let wasm = wat::parse_str(
        r#"
        (module
            (func (export "sw") (param i32) (result i32)
                (block $0 (result i32)
                    (block $1
                        (block $2
                            (block $3
                                (br_table $1 $2 $3 (local.get 0))
                            )
                            (return (i32.const 300))
                        )
                        (return (i32.const 200))
                    )
                    (return (i32.const 100))
                )
            )
        )
    "#,
    )
    .unwrap();

    let mut reader = washm::wasm::binary_reader::BinaryReader::new();
    let mut module = reader.parse_module(&wasm).expect("parse module");
    let mut lifter = washm::decompile::Lifter::default();
    lifter.run(&mut module);

    let mut printer = washm::decompile::printers::RustPrinter::new(&module);
    let output = printer.print();
    println!("DEBUG OUTPUT:\n{}", output);

    // We expect a match
    assert!(output.contains("match"), "Output should contain match");
}
#[test]
fn test_loop_with_break() {
    let wasm = wat::parse_str(
        r#"
        (module
            (func (export "lp") (param i32) (result i32)
                (loop $top (result i32)
                    (local.get 0)
                    (br_if $top (i32.const 1))
                )
            )
        )
    "#,
    )
    .unwrap();

    let mut reader = washm::wasm::binary_reader::BinaryReader::new();
    let mut module = reader.parse_module(&wasm).expect("parse module");
    let mut lifter = washm::decompile::Lifter::default();
    lifter.run(&mut module);

    let mut printer = washm::decompile::printers::RustPrinter::new(&module);
    let output = printer.print();
    println!("LOOP OUTPUT:\n{}", output);

    assert!(output.contains("loop"), "Output should contain loop");
}

#[test]
fn test_if_else_reconstruction() {
    let wasm = wat::parse_str(
        r#"
        (module
            (import "env" "side_effect" (func $se (param i32)))
            (func (export "ie") (param i32)
                (if (local.get 0)
                    (then (call $se (i32.const 1)))
                    (else (call $se (i32.const 2)))
                )
            )
        )
    "#,
    )
    .unwrap();

    let mut reader = washm::wasm::binary_reader::BinaryReader::new();
    let mut module = reader.parse_module(&wasm).expect("parse module");

    let mut lifter = washm::decompile::Lifter::default();
    lifter.run(&mut module);

    let mut printer = washm::decompile::printers::RustPrinter::new(&module);
    let output = printer.print();
    println!("IF-ELSE OUTPUT:\n{}", output);

    assert!(
        output.contains("if") && output.contains("else"),
        "Output should contain if-else"
    );
}

#[test]
fn test_parse_if_else_leak_isolation() {
    let wasm = wat::parse_str(
        r#"
        (module
            (func $ie (param i32)
                (local.get 0)
                (if
                    (then
                        (nop)
                    )
                    (else
                        (nop)
                    )
                )
            )
        )
    "#,
    )
    .unwrap();

    let mut reader = washm::wasm::binary_reader::BinaryReader::new();
    let module = reader.parse_module(&wasm).expect("parse module");

    // We check the IR for the "ie" function
    for (_, func) in &module.functions {
        if func.name == "ie" {
            if let washm::ir::module::FunctionKind::Local { body, .. } = &func.kind {
                use washm::ir::expression::ExpressionKind;
                let top_block = &module.expressions[*body];
                if let ExpressionKind::Block { list, .. } = &top_block.kind {
                    // Check for LEAK. The list should only have 1 item: the If wrapper.
                    // If we see 2 extra Nops (from then/else) + our If, that's a leak.
                    assert_eq!(
                        list.len(),
                        1,
                        "Leaked instructions found in top-level block. Expected 1, found {}: {:?}",
                        list.len(),
                        list.iter()
                            .map(|id| format!("{:?}", module.expressions[*id].kind))
                            .collect::<Vec<_>>()
                    );
                } else {
                    panic!("Expected top-level block");
                }
            }
        }
    }
}

#[test]
fn test_struct_access() {
    common::run_roundtrip_test_rust("tests/roundtrip/_struct.rs");
}
