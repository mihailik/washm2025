#[test]
fn parse_module_without_tags_has_empty_tags() {
    let parser = crate::Parser::new();
    let wat = r#"(module (func))"#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    assert_eq!(module.tags.len(), 0usize);
}
