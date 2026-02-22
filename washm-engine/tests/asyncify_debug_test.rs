use washm::ir::annotation::DebugLocation;
use washm::passes::asyncify::{Asyncify, AsyncifyConfig};
use washm::passes::Pass;
use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;

#[test]
fn test_asyncify_preserves_debug_location() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main
                (call $sleep)
            )
        )
    "#;
    let wasm_bytes = wat::parse_str(wat).expect("failed to parse wat");

    let mut reader = BinaryReader::new();
    let mut module = reader
        .parse_module(&wasm_bytes)
        .expect("failed to read module");

    // Find the call expression and annotate it
    let call_expr_id = module
        .expressions
        .iter()
        .find(|(_, expr)| {
            matches!(
                expr.kind,
                washm::ir::expression::ExpressionKind::Call { .. }
            )
        })
        .expect("call not found")
        .0;

    let test_location = DebugLocation {
        file_index: 1,
        line: 42,
        column: 10,
    };
    module.annotations.set_location(call_expr_id, test_location);

    let config = AsyncifyConfig {
        imports: vec!["env.sleep".to_string()],
        ..Default::default()
    };
    let mut asyncify = Asyncify::new(config);
    asyncify.run(&mut module);

    // After asyncify, the original call should still have its location.
    // Also, the wrapping 'if' should have inherited the location.

    let loc = module.annotations.get_location(call_expr_id);
    assert_eq!(loc, Some(test_location), "Original call location lost");

    // Check if any new expressions (like the 'if' wrapper) have this location.
    // Asyncify wraps the call in an 'if' that also gets the location.
    let wrapping_if = module.expressions.iter().find(|(id, expr)| {
        if let washm::ir::expression::ExpressionKind::If { .. } = &expr.kind {
            module.annotations.get_location(*id) == Some(test_location)
        } else {
            false
        }
    });

    assert!(
        wrapping_if.is_some(),
        "Wrapping 'if' did not inherit location"
    );
}
