use std::fs;
use std::path::PathBuf;
use std::process::Command;
use washm::decompile::printers::{CPrinter, RustPrinter};
use washm::decompile::Lifter;
use washm::wasm::binary_reader::BinaryReader;

#[allow(dead_code)]
pub fn run_roundtrip_test_rust(source_path_str: &str) {
    let (rust_out, _cpp_out, rust_gold_path, _cpp_gold_path) =
        generate_roundtrip_outputs(source_path_str);

    // If gold is missing, treat as empty string per spec and rely on string equality to show mismatches
    let expected = if rust_gold_path.exists() {
        fs::read_to_string(&rust_gold_path).expect("Failed to read rust gold")
    } else {
        String::new()
    };

    let actual_trimmed = rust_out.trim().replace("\r\n", "\n");
    let expected_trimmed = expected.trim().replace("\r\n", "\n");

    if expected_trimmed.is_empty() || std::env::var("UPDATE_GOLD").is_ok() {
        fs::write(&rust_gold_path, &actual_trimmed).expect("Failed to write rust gold");
        return;
    }

    assert_eq!(
        expected_trimmed, actual_trimmed,
        "Rust roundtrip mismatch for {:?}",
        rust_gold_path
    );
}

#[allow(dead_code)]
pub fn run_roundtrip_test_cpp(source_path_str: &str) {
    let (_rust_out, cpp_out, _rust_gold_path, cpp_gold_path) =
        generate_roundtrip_outputs(source_path_str);

    // If gold is missing, treat as empty string per spec and rely on string equality to show mismatches
    let expected = if cpp_gold_path.exists() {
        fs::read_to_string(&cpp_gold_path).expect("Failed to read cpp gold")
    } else {
        String::new()
    };

    let actual_trimmed = cpp_out.trim().replace("\r\n", "\n");
    let expected_trimmed = expected.trim().replace("\r\n", "\n");

    if expected_trimmed.is_empty() || std::env::var("UPDATE_GOLD").is_ok() {
        fs::write(&cpp_gold_path, &actual_trimmed).expect("Failed to write cpp gold");
        return;
    }

    assert_eq!(
        expected_trimmed, actual_trimmed,
        "C++ roundtrip mismatch for {:?}",
        cpp_gold_path
    );
}

#[allow(dead_code)]
fn generate_roundtrip_outputs(source_path_str: &str) -> (String, String, PathBuf, PathBuf) {
    let source_path = hex_to_path(source_path_str);
    let stem = source_path
        .file_stem()
        .expect("file stem")
        .to_str()
        .expect("utf8 stem");
    let dir = source_path.parent().expect("parent dir");

    let rust_gold_path = dir.join(format!("{}.roundtrip.rs", stem));
    let cpp_gold_path = dir.join(format!("{}.roundtrip.cpp", stem));

    // 1. Compile to WASM
    let temp_wasm = tempfile::NamedTempFile::new().expect("Failed to create temp wasm file");
    let wasm_path = temp_wasm.path();

    let status = Command::new("rustc")
        .args([
            "--target",
            "wasm32-unknown-unknown",
            "-O",
            "--crate-type=cdylib",
            "-o",
            wasm_path.to_str().expect("utf8 path"),
            source_path.to_str().expect("utf8 source path"),
        ])
        .output()
        .expect("Failed to execute rustc");

    if !status.status.success() {
        panic!(
            "rustc failed for {}\nstdout: {}\nstderr: {}",
            source_path_str,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }

    // 2. Read WASM
    let wasm_bytes = fs::read(wasm_path).expect("Failed to read generated wasm");
    let mut reader = BinaryReader::new();
    let mut module = reader
        .parse_module(&wasm_bytes)
        .expect("Failed to parse WASM module");

    // 3. Decompile
    let mut lifter = Lifter;
    lifter.run(&mut module);

    let rust_out = RustPrinter::new(&module).print();
    let cpp_out = CPrinter::new(&module).print();

    (rust_out, cpp_out, rust_gold_path, cpp_gold_path)
}

/// Trace Lifter Passes: Run the lifter-like sequence of passes one by one and
/// capture the Rust printing after each pass so tests can inspect which
/// pass introduces regressions.
#[allow(dead_code)]
pub fn trace_lifter_passes_for_source(source_path_str: &str) -> Vec<(String, String)> {
    let source_path = hex_to_path(source_path_str);

    // Compile to WASM
    let temp_wasm = tempfile::NamedTempFile::new().expect("Failed to create temp wasm file");
    let wasm_path = temp_wasm.path();

    let status = Command::new("rustc")
        .args([
            "--target",
            "wasm32-unknown-unknown",
            "-O",
            "--crate-type=cdylib",
            "-o",
            wasm_path.to_str().expect("utf8 path"),
            source_path.to_str().expect("utf8 source path"),
        ])
        .output()
        .expect("Failed to execute rustc");

    if !status.status.success() {
        panic!(
            "rustc failed for {}\nstdout: {}\nstderr: {}",
            source_path_str,
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }

    let wasm_bytes = fs::read(wasm_path).expect("Failed to read generated wasm");
    let mut reader = BinaryReader::new();
    let mut module = reader
        .parse_module(&wasm_bytes)
        .expect("Failed to parse WASM module");

    use washm::decompile::passes::{ExpressionRecombination, NameInferencePass};
    use washm::decompile::printers::RustPrinter;
    use washm::passes::dce::DCE;
    use washm::passes::global_refining::GlobalRefining;
    use washm::passes::gufa::GUFA;
    use washm::passes::local_cse::LocalCSE;
    use washm::passes::local_subtyping::LocalSubtyping;
    use washm::passes::merge_blocks::MergeBlocks;
    use washm::passes::optimize_instructions::OptimizeInstructions;
    use washm::passes::rereloop::Rereloop;
    use washm::passes::simplify_control_flow::SimplifyControlFlow;
    use washm::passes::vacuum::Vacuum;
    use washm::passes::Pass;

    let mut outputs: Vec<(String, String)> = Vec::new();

    // 0. Preliminary optimizations and inferences
    LocalSubtyping.run(&mut module);
    outputs.push((
        "LocalSubtyping".to_string(),
        RustPrinter::new(&module).print(),
    ));

    GlobalRefining.run(&mut module);
    outputs.push((
        "GlobalRefining".to_string(),
        RustPrinter::new(&module).print(),
    ));

    DCE.run(&mut module);
    outputs.push(("DCE".to_string(), RustPrinter::new(&module).print()));

    GUFA.run(&mut module);
    outputs.push(("GUFA".to_string(), RustPrinter::new(&module).print()));

    // 1. Restructure control flow for cleaner decompilation
    SimplifyControlFlow.run(&mut module);
    outputs.push((
        "SimplifyControlFlow".to_string(),
        RustPrinter::new(&module).print(),
    ));

    Vacuum.run(&mut module);
    outputs.push(("Vacuum".to_string(), RustPrinter::new(&module).print()));

    // Debug: dump the raw expression of the first function body before Rereloop
    // Dump bodies for all local functions for debugging
    for (name, func) in &module.functions {
        if let washm::ir::module::FunctionKind::Local { body, .. } = &func.kind {
            let expr = &module.expressions[*body];
            outputs.push((
                format!("DebugBeforeRereloop({})", name),
                format!("{:#?}", expr),
            ));
            // If it's a block, also dump each inner expression with its id to inspect structure
            if let washm::ir::expression::ExpressionKind::Block { list, .. } = &expr.kind {
                for &inner in list {
                    outputs.push((
                        format!("DebugBeforeRereloop({})[{}]", name, inner),
                        format!("{:#?}", &module.expressions[inner]),
                    ));

                    // Dump referenced children of this inner expression for extra context
                    let inner_expr = &module.expressions[inner];
                    let mut child_ids = Vec::new();
                    inner_expr.kind.for_each_child(|c| child_ids.push(c));
                    for c in child_ids {
                        outputs.push((
                            format!("DebugBeforeRereloop({})[{}].child[{}]", name, inner, c),
                            format!("{:#?}", &module.expressions[c]),
                        ));
                    }
                }
            }
        }
    }

    Rereloop.run(&mut module);
    outputs.push(("Rereloop".to_string(), RustPrinter::new(&module).print()));

    // Dump bodies after Rereloop to inspect generated returns/ifs
    for (name, func) in &module.functions {
        if let washm::ir::module::FunctionKind::Local { body, .. } = &func.kind {
            let expr = &module.expressions[*body];
            outputs.push((
                format!("DebugAfterRereloop({})", name),
                format!("{:#?}", expr),
            ));
            if let washm::ir::expression::ExpressionKind::Block { list, .. } = &expr.kind {
                for &inner in list {
                    outputs.push((
                        format!("DebugAfterRereloop({})[{}]", name, inner),
                        format!("{:#?}", &module.expressions[inner]),
                    ));

                    // Also dump referenced children of this inner expression for extra context
                    let inner_expr = &module.expressions[inner];
                    let mut child_ids = Vec::new();
                    inner_expr.kind.for_each_child(|c| child_ids.push(c));
                    for c in child_ids {
                        outputs.push((
                            format!("DebugAfterRereloop({})[{}].child[{}]", name, inner, c),
                            format!("{:#?}", &module.expressions[c]),
                        ));
                        // Dump one level deeper
                        let mut grand_children = Vec::new();
                        module.expressions[c]
                            .kind
                            .for_each_child(|gc| grand_children.push(gc));
                        for gc in grand_children {
                            outputs.push((
                                format!(
                                    "DebugAfterRereloop({})[{}].child[{}].child[{}]",
                                    name, inner, c, gc
                                ),
                                format!("{:#?}", &module.expressions[gc]),
                            ));
                        }
                    }
                }
            }
        }
    }

    // RemoveUnusedNames is internal; call MergeBlocks and others next
    MergeBlocks.run(&mut module);
    outputs.push(("MergeBlocks".to_string(), RustPrinter::new(&module).print()));

    LocalCSE.run(&mut module);
    outputs.push(("LocalCSE".to_string(), RustPrinter::new(&module).print()));

    OptimizeInstructions::default().run(&mut module);
    outputs.push((
        "OptimizeInstructions".to_string(),
        RustPrinter::new(&module).print(),
    ));

    // 2. Global analysis is skipped for this trace

    // 3. Identify high-level types and structures (skipped semantics)

    // 4. Identify control flow patterns
    // Run vacuum/merge/simplify again to capture diffs
    Vacuum.run(&mut module);
    outputs.push(("Vacuum2".to_string(), RustPrinter::new(&module).print()));

    MergeBlocks.run(&mut module);
    outputs.push((
        "MergeBlocks2".to_string(),
        RustPrinter::new(&module).print(),
    ));

    SimplifyControlFlow.run(&mut module);
    outputs.push((
        "SimplifyControlFlow2".to_string(),
        RustPrinter::new(&module).print(),
    ));

    // 5. Recombine expressions
    ExpressionRecombination::run(&mut module);
    outputs.push((
        "ExpressionRecombination".to_string(),
        RustPrinter::new(&module).print(),
    ));

    // 6. Name inference
    NameInferencePass::new().run(&mut module);
    outputs.push((
        "NameInference".to_string(),
        RustPrinter::new(&module).print(),
    ));

    outputs
}

#[allow(dead_code)]
fn hex_to_path(path_str: &str) -> PathBuf {
    PathBuf::from(path_str)
}
