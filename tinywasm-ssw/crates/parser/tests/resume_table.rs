mod util;

#[cfg(feature = "stack-switching")]
#[test]
fn parse_simple_tiny_module_has_empty_resume_tables() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"(module (func))"#;
    let wasm = util::parse_wast_to_wasm_bytes(wat);
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    // If stack-switching is enabled, resume_tables present; default: should be empty
    #[cfg(feature = "stack-switching")]
    assert_eq!(module.funcs[0].data.resume_tables.len(), 0usize);
}
