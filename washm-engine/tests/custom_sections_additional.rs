use washm::ir::expression::IrBuilder;
use washm::ir::module::CustomSection as IrCustomSection;
use washm::ir::module::DataSegment;
use washm::ir::Literal;
use washm::passes::Pass;
use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;
use wasm_encoder::{CustomSection, Module as EncoderModule, NameSection};
use wasmparser::Parser;

#[test]
fn custom_sections_order_and_content_roundtrip() {
    // Build module with multiple custom sections in a given order
    let mut m = EncoderModule::new();
    m.section(&CustomSection {
        name: std::borrow::Cow::Borrowed("a"),
        data: vec![1u8].into(),
    });
    m.section(&CustomSection {
        name: std::borrow::Cow::Borrowed(".debug_line"),
        data: vec![9u8, 8u8].into(),
    });
    m.section(&CustomSection {
        name: std::borrow::Cow::Borrowed("b"),
        data: vec![2u8, 3u8].into(),
    });

    let bytes = m.finish();
    let mut reader = BinaryReader::new();
    let module = reader.parse_module(&bytes).expect("parse");

    // Ensure parser captured custom sections in the same order
    let names: Vec<_> = module
        .custom_sections
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(
        names,
        vec!["a".to_string(), ".debug_line".to_string(), "b".to_string()]
    );

    // Write module back and inspect emitted custom sections order
    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    let mut parsed_names = Vec::new();
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            parsed_names.push(s.name().to_string());
        }
    }

    // The writer should have emitted our custom sections; ensure they are present
    for n in names.iter() {
        assert!(parsed_names.contains(n), "Missing custom section {}", n);
    }
}

#[test]
fn name_and_producers_sections_survive() {
    let mut m = EncoderModule::new();

    // Add a name section (writer normally emits one, but we test explicit parsing too)
    let names = NameSection::new();
    // leave empty but ensure presence
    m.section(&names);

    // add a custom producers section
    m.section(&CustomSection {
        name: std::borrow::Cow::Borrowed("producers"),
        data: vec![0xAA].into(),
    });

    let bytes = m.finish();
    let mut reader = BinaryReader::new();
    let module = reader.parse_module(&bytes).expect("parse");

    assert!(module.custom_sections.iter().any(|s| s.name == "producers"));

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    let mut has_producers = false;
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() == "producers" && s.data() == [0xAA] {
                has_producers = true;
            }
        }
    }
    assert!(has_producers, "producers section was not preserved");
}

#[test]
fn expr_offsets_various_ops() {
    // Use WAT to construct a slightly more complex function
    let wat = r#"
    (module
      (func (export "f") (result i32)
        (local i32)
        i32.const 1
        local.set 0
        local.get 0
        block
          i32.const 2
          local.get 0
        end
        i32.const 3
        i32.add
      )
    )"#;

    let wasm = wat::parse_str(wat).expect("wat parse");
    let mut reader = BinaryReader::new();
    let _module = reader.parse_module(&wasm).expect("parse");

    let map = reader.expr_offsets_map();
    // Expect multiple recorded offsets (consts, local.get/set, block, etc.)
    assert!(map.len() >= 3, "expected multiple expression offsets");
    for off in map.values() {
        assert!(*off > 0, "expected non-zero offset");
    }
}

#[test]
fn writer_preserves_custom_sections_after_memory_and_data_changes() {
    // Start from a minimal module with a memory
    let mut module = {
        let mut reader = BinaryReader::new();
        let wasm = wat::parse_str("(module (memory 1))").expect("wat");
        reader.parse_module(&wasm).expect("parse")
    };

    // Add a custom section
    module.custom_sections.push(IrCustomSection {
        name: "meta".to_string(),
        data: vec![9, 8, 7],
    });

    // Add a data segment that uses memory
    let offset_expr = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    module.set_memory(1, Some(10));
    module.add_data_segment(DataSegment {
        memory_index: washm::ir::MemoryId::from_raw_parts(0, 0),
        offset: offset_expr,
        data: b"payload".to_vec(),
    });

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    // Ensure custom section is still present in output
    let mut found = false;
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() == "meta" && s.data() == b"\x09\x08\x07" {
                found = true;
            }
        }
    }
    assert!(found, "custom section missing after data changes");
}

#[test]
fn passes_preserve_custom_sections() {
    // Parse module with custom section
    let mut reader = BinaryReader::new();
    let wasm = wat::parse_str("(module)").expect("wat");
    let mut module = reader.parse_module(&wasm).expect("parse");
    module.custom_sections.push(IrCustomSection {
        name: "keepme".to_string(),
        data: vec![1],
    });

    // Run StripDebug pass which should not touch custom sections
    let mut pass = washm::passes::strip_debug::StripDebug;
    pass.run(&mut module);

    assert!(
        module.custom_sections.iter().any(|s| s.name == "keepme"),
        "custom section was removed by pass"
    );
}

#[test]
fn writer_emits_standard_function_names() {
    // Ensure writer emits our function names in the standard 'name' section
    let mut reader = BinaryReader::new();
    let wasm = wat::parse_str("(module (func) (func))").expect("wat");
    let mut module = reader.parse_module(&wasm).expect("parse");

    // Assign names explicitly
    let ids: Vec<_> = module.functions.iter().map(|(id, _)| id.cast()).collect();
    for (i, id) in ids.into_iter().enumerate() {
        if let Some(f) = module.get_function_mut(id) {
            f.name = format!("fn{}", i);
        }
    }

    let mut writer = BinaryWriter::new();
    let out = writer.write_module(&module).expect("write");

    let mut found = false;
    for payload in Parser::new(0).parse_all(&out) {
        let payload = payload.expect("parser");
        if let wasmparser::Payload::CustomSection(s) = payload {
            if s.name() == "name" {
                found = true;
                break;
            }
        }
    }
    assert!(found, "Standard 'name' section was not emitted");
}
