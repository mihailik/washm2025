use std::fs;
use std::path::PathBuf;
use std::process::Command;
use washm::decompile::printers::{CPrinter, RustPrinter};
use washm::decompile::Lifter;
use washm::wasm::binary_reader::BinaryReader;

pub fn run_triad_test_rust(source_path_str: &str) {
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

    assert_eq!(
        expected_trimmed, actual_trimmed,
        "Rust roundtrip mismatch for {:?}",
        rust_gold_path
    );
}

pub fn run_triad_test_cpp(source_path_str: &str) {
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

    assert_eq!(
        expected_trimmed, actual_trimmed,
        "C++ roundtrip mismatch for {:?}",
        cpp_gold_path
    );
}

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
    let mut lifter = Lifter::default();
    lifter.run(&mut module);

    let rust_out = RustPrinter::new(&module).print();
    let cpp_out = CPrinter::new(&module).print();

    (rust_out, cpp_out, rust_gold_path, cpp_gold_path)
}

fn hex_to_path(path_str: &str) -> PathBuf {
    PathBuf::from(path_str)
}
