#[test]
fn parse_simple_tiny_module_has_empty_resume_tables() {
    let parser = crate::Parser::new();
    let wat = r#"(module (func))"#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    // If stack-switching is enabled, resume_tables present; default: should be empty
    #[cfg(feature = "stack-switching")]
    assert_eq!(module.funcs[0].data.resume_tables.len(), 0usize);
}
