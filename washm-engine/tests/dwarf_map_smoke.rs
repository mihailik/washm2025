use washm::ir::module::CustomSection as IrCustomSection;
use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;
use wasm_encoder::{CustomSection, Module as EncoderModule};
use wasmparser::Parser;

#[test]
fn dwarf_map_smoke() {
    // 1) Parse a minimal module to get a recorded expression offset
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
    let module = reader.parse_module(&wasm).expect("parse");

    // pick any recorded expr offset
    let some_offset = *reader
        .expr_offsets_map()
        .values()
        .next()
        .expect("no expr offsets recorded");

    // 2) Inject a synthetic .debug_line.map into the module and re-emit bytes
    let mut module2 = module;
    let mut map_bytes = Vec::new();
    // write one mapping: offset, file_index=1, line=123, column=0
    map_bytes.extend(&some_offset.to_le_bytes());
    map_bytes.extend(&1u32.to_le_bytes());
    map_bytes.extend(&123u32.to_le_bytes());
    map_bytes.extend(&0u32.to_le_bytes());

    module2.custom_sections.push(IrCustomSection {
        name: ".debug_line.map".to_string(),
        data: map_bytes,
    });

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module2).expect("write");

    // 3) Parse the re-emitted module and verify annotations were applied
    let mut reader2 = BinaryReader::new();
    let module3 = reader2.parse_module(&out).expect("parse");

    // Find an expression with DebugLocation annotation
    let mut found = false;
    for (expr_id, ann) in module3.annotations.clone().into_iter() {
        if let Some(loc) = ann.location {
            if loc.line == 123 {
                found = true;
                break;
            }
        }
    }
    assert!(found, "DWARF map prototype did not annotate any expression");
}
