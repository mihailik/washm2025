#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_cont_new_has_instruction() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"(module (type (func)) (func (type 0) (cont.new 0)))"#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        use tinywasm_types::Instruction;
        let func = &module.funcs[0];
        assert!(func.instructions.iter().any(|i| matches!(i, Instruction::ContNew(0))));
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_cont_bind_has_instruction() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func (param i32) (result i32)))
            (func (type 0)
                (local i32)
                (local.set 0 (i32.const 1))
                (cont.new 0)
                (cont.bind 0 0)
            )
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        use tinywasm_types::Instruction;
        let func = &module.funcs[0];
        assert!(func.instructions.iter().any(|i| matches!(i, Instruction::ContBind(0, 0))));
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_suspend_has_instruction() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func (param i32)))
            (tag (type 0))
            (func (type 0) (suspend 0))
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        use tinywasm_types::Instruction;
        let func = &module.funcs[0];
        assert!(func.instructions.iter().any(|i| matches!(i, Instruction::Suspend(0))));
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_switch_has_instruction() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func))
            (tag (type 0))
            (func (type 0) (switch 0 0))
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        use tinywasm_types::Instruction;
        let func = &module.funcs[0];
        assert!(func.instructions.iter().any(|i| matches!(i, Instruction::Switch(0, 0))));
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_resume_has_resume_table_and_instruction() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func))
            (func (type 0) (resume 0 (resume_table 0)))
            (func (type 0))
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        use tinywasm_types::Instruction;
        let func = &module.funcs[0];
        assert_eq!(func.data.resume_tables.len(), 1usize);
        assert!(func.instructions.iter().any(|i| matches!(i, Instruction::Resume(_, 0))));
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_module_with_two_resume_instructions_produces_two_resume_tables() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func))
            (func (type 0) (resume 0 (resume_table 0)) (resume 0 (resume_table 0)))
            (func (type 0))
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let module = parser.parse_module_bytes(&wasm).expect("parse module");
    #[cfg(feature = "stack-switching")]
    {
        let func = &module.funcs[0];
        assert_eq!(func.data.resume_tables.len(), 2usize);
        // ensure the instructions contain resume indices 0 and 1
        use tinywasm_types::Instruction;
        let mut found0 = false;
        let mut found1 = false;
        for i in func.instructions.iter() {
            if let Instruction::Resume(_, idx) = i {
                if *idx == 0 { found0 = true; }
                if *idx == 1 { found1 = true; }
            }
        }
        assert!(found0 && found1);
    }
}

#[cfg(feature = "stack-switching")]
#[test]
fn parse_cont_bind_out_of_range_is_rejected() {
    let parser = tinywasm_parser::Parser::new();
    let wat = r#"
        (module
            (type (func (param i32) (result i32)))
            (func (type 0)
                (local i32)
                (cont.new 0)
                ;; intentionally out-of-range indices (large)
                (cont.bind 999 1000)
            )
        )
    "#;
    let wasm = wat::parse_str(wat).expect("parse wat");
    let result = parser.parse_module_bytes(&wasm);
    assert!(result.is_err());
}


