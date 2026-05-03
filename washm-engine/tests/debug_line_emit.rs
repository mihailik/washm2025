use washm::ir::annotation::DebugLocation;
use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;
use wasmparser::Parser;

#[test]
fn debug_line_emitted() {
    // Minimal module with a function that returns i32.const 42
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
    let mut module = reader.parse_module(&wasm).expect("parse");

    // pick an arbitrary expression to annotate: the first expression in the arena
    let some_expr = module
        .expressions
        .iter()
        .next()
        .expect("no expressions in module")
        .0;

    module.annotations.set_location(
        some_expr,
        DebugLocation {
            file_index: 1,
            line: 123,
            column: 0,
        },
    );

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    // Ensure .debug_line exists
    let mut found = false;
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() == ".debug_line" {
                found = true;
                assert!(!s.data().is_empty(), ".debug_line section is empty");
            }
        }
    }
    assert!(found, ".debug_line custom section not emitted");
}
