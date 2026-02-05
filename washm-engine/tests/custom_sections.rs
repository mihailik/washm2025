use wasm_encoder::CustomSection;
use wasm_encoder::Module as EncoderModule;
use wasmparser::Parser;

use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;

#[test]
fn reader_captures_custom_sections() {
    // Build a minimal wasm module with a custom section
    let mut m = EncoderModule::new();
    let section = CustomSection {
        name: std::borrow::Cow::Borrowed("test.sec"),
        data: vec![1, 2, 3].into(),
    };
    m.section(&section);
    let bytes = m.finish();

    let mut reader = BinaryReader::new();
    let module = reader.parse_module(&bytes).expect("parse");

    let found = module
        .custom_sections
        .iter()
        .any(|s| s.name == "test.sec" && s.data == vec![1, 2, 3]);
    assert!(found, "Custom section was not captured by BinaryReader");
}

#[test]
fn writer_emits_custom_sections_roundtrip() {
    // Start from a parsed module, add a custom section, write it out and ensure parser sees it
    let mut m = EncoderModule::new();
    // add a trivial function so module is valid
    m.section(&wasm_encoder::TypeSection::new());
    let bytes = m.finish();

    let mut reader = BinaryReader::new();
    let mut module = reader.parse_module(&bytes).expect("parse");
    module
        .custom_sections
        .push(washm::ir::module::CustomSection {
            name: "meta".to_string(),
            data: vec![9, 8, 7],
        });

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    // Parse output bytes and assert custom section exists
    let mut found = false;
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() == "meta" && s.data() == &[9, 8, 7] {
                found = true;
                break;
            }
        }
    }
    assert!(found, "Custom section was not emitted by BinaryWriter");
}

#[test]
fn reader_records_expr_offsets() {
    // Minimal module: one function with i32.const 42
    let wasm = vec![
        0x00, 0x61, 0x73, 0x6D, // magic
        0x01, 0x00, 0x00, 0x00, // version
        // Type section
        0x01, 0x05, // section 1, size 5
        0x01, // 1 type
        0x60, 0x00, 0x01, 0x7F, // func type: () -> i32
        // Function section
        0x03, 0x02, // section 3, size 2
        0x01, 0x00, // 1 function, type 0
        // Code section
        0x0A, 0x06, // section 10, size 6
        0x01, // 1 code
        0x04, // body size 4
        0x00, // 0 locals
        0x41, 0x2A, // i32.const 42
        0x0B, // end
    ];

    let mut reader = BinaryReader::new();
    let _module = reader.parse_module(&wasm).expect("parse");

    let map = reader.expr_offsets_map();
    // There should be at least one recorded expression offset and values should be non-zero
    assert!(!map.is_empty(), "expr_offsets map is empty");
    let mut has_non_zero = false;
    for (_expr, off) in map.iter() {
        if *off > 0 {
            has_non_zero = true;
            break;
        }
    }
    assert!(has_non_zero, "no non-zero expression offsets recorded");
}
