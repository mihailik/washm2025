mod common;

use std::process::Command;
use washm::decompile::Lifter;
use washm::ir::expression::ExpressionKind;
use washm::wasm::binary_reader::BinaryReader;

#[test]
fn memcopy_survives_rereloop() {
    // Compile the Rust fixture to wasm (same as other roundtrip tests)
    let source_path = std::env::current_dir()
        .unwrap()
        .join("tests/roundtrip/_memcopy.rs");
    let temp_wasm = tempfile::NamedTempFile::new().expect("temp wasm");
    let wasm_path = temp_wasm.path();

    let out = Command::new("rustc")
        .args(&[
            "--target",
            "wasm32-unknown-unknown",
            "-O",
            "--crate-type=cdylib",
            "-o",
            wasm_path.to_str().unwrap(),
            source_path.to_str().unwrap(),
        ])
        .output()
        .expect("rustc failed");
    assert!(
        out.status.success(),
        "rustc failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let wasm_bytes = std::fs::read(wasm_path).expect("read wasm");
    let mut reader = BinaryReader::new();
    let mut module = reader.parse_module(&wasm_bytes).expect("parse");

    // Run lifter (full pipeline used by tests)
    let mut lifter = Lifter::default();
    lifter.run(&mut module);

    // Search for MemoryCopy reachable from the function body
    let mut found = false;
    for (_id, func) in module.functions.iter() {
        if let Some(body_id) = func.body() {
            if has_memory_copy(&module.expressions, body_id) {
                found = true;
                break;
            }
        }
    }

    assert!(found, "MemoryCopy was removed by passes (regression)");
}

fn has_memory_copy(
    arena: &washm::collections::IdArena<
        washm::ir::expression::ExpressionMarker,
        washm::ir::expression::Expression,
    >,
    id: washm::ir::expression::ExpressionId,
) -> bool {
    let expr = &arena[id];
    if matches!(expr.kind, ExpressionKind::MemoryCopy { .. }) {
        return true;
    }
    let mut found = false;
    expr.kind.for_each_child(|child| {
        if has_memory_copy(arena, child) {
            found = true;
        }
    });
    found
}
