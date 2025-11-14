mod util;

#[cfg(feature = "stack-switching")]
#[test]
fn parse_resume_table_addrs_resolved_correctly() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func (param i32)))
            (func (type 0) (local i64) (resume 0 (resume_table 0)))
        )
    "#;
    let wasm = util::parse_wast_to_wasm_bytes(wat);
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    let func = &module.funcs[0];
    // resume_tables contains wasm local indices
    assert_eq!(func.data.resume_tables.len(), 1usize);
    // resume_table_addrs contains resolved local addresses (LocalAddr).
    assert_eq!(func.data.resume_table_addrs.len(), 1usize);
    // For this function, wasm local index 0 (param i32) resolves to local address 0.
    assert_eq!(func.data.resume_table_addrs[0][0], 0u16);
}
