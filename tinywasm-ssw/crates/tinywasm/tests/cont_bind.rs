mod testsuite;

#[cfg(feature = "stack-switching")]
mod cont_bind_tests {
    use eyre::Result;
    use tinywasm_types::WasmValue;
    use crate::testsuite::util;

    #[test]
    fn runtime_cont_bind_basic_returns_bound_value() -> Result<()> {
        // Create a wat module that constructs a continuation for a second function and binds
        // the first function's param into the target function's param, then resumes.
        let wat = r#"
            (module
                (type (func (param i32) (result i32)))
                (func (type 0) (param i32) (result i32) ;; target function (index 1)
                    local.get 0
                )
                (func (type 0) (param i32) (result i32) ;; main function (index 0)
                    (local.get 0)
                    (ref.func 1)
                    (cont.new 0)
                    (cont.bind 0 0)
                    (resume 0 (resume_table 0))
                )
                (export "main" (func 0))
            )
        "#;

        let lexer = wasm_testsuite::wast::lexer::Lexer::new(wat);
        let buf = wasm_testsuite::wast::parser::ParseBuffer::new_with_lexer(lexer).expect("failed to create parse buffer");
        let mut wat_data = wasm_testsuite::wast::parser::parse::<wasm_testsuite::wast::Wat>(&buf).expect("failed to parse wat");
        let wasm = wat_data.encode().expect("failed to encode module");
        let module = util::parse_module_bytes(&wasm)?;
        let res = util::exec_fn(Some(&module), "main", &[WasmValue::I32(42)], None)?;
        assert_eq!(res, vec![WasmValue::I32(42)]);
        Ok(())
    }

    #[test]
    fn runtime_cont_bind_out_of_range_result_index_is_trap() {
        // Attempt a cont.bind with result index out of range for the resumed function
        let wat = r#"
            (module
                (type (func (param i32) (result i32)))
                (func (type 0) (param i32) (result i32)
                    local.get 0
                )
                (func (type 0) (param i32) (result i32)
                    (local.get 0)
                    (ref.func 1)
                    (cont.new 0)
                    ;; bind to result index 999 which is out of range
                    (cont.bind 0 999)
                    (resume 0 (resume_table 0))
                )
                (export "main" (func 0))
            )
        "#;

        let lexer = wasm_testsuite::wast::lexer::Lexer::new(wat);
        let buf = wasm_testsuite::wast::parser::ParseBuffer::new_with_lexer(lexer).expect("failed to create parse buffer");
        let mut wat_data = wasm_testsuite::wast::parser::parse::<wasm_testsuite::wast::Wat>(&buf).expect("failed to parse wat");
        let wasm = wat_data.encode().expect("failed to encode module");
        let module = util::parse_module_bytes(&wasm).expect("parse module");
        let res = util::exec_fn(Some(&module), "main", &[WasmValue::I32(1)], None);
        assert!(res.is_err());
    }
}
