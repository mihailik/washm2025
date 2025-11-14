mod util;

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_without_tags_has_empty_tags() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"(module (func))"#;
    let wasm = util::parse_wast_to_wasm_bytes(wat);
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    assert_eq!(module.tags.len(), 0usize);
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_tag_type_params() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func (param i32)))
            (tag (type 0))
        )
    "#;
    let wasm = util::parse_wast_to_wasm_bytes(wat);
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        assert_eq!(module.tags.len(), 1usize);
        use tinywasm_types::ValType;
        assert_eq!(module.tags[0].params.as_ref(), [ValType::I32].as_ref());
    }
}
