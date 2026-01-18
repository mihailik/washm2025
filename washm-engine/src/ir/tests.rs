#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_validation_failure() {
        let bump = Bump::new();
        let _module_name = "test_module";

        // Create mismatched binary op: i32 + f32
        let left = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(1)),
            type_: Type::I32,
        }));
        let right = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::F32(2.0)),
            type_: Type::F32,
        }));

        let binary_expr = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::AddInt32, // Note: using int32 add
                left,
                right,
            },
            type_: Type::I32, // Result type claimed to be i32
        }));

        let functions = vec![Function {
            name: "bad_func".to_string(),
            type_idx: None,
            params: Type::NONE,
            results: Type::I32,
            vars: Vec::new(),
            body: Some(binary_expr),
            local_names: vec![],
        }];

        let module = Module {
            type_names: Vec::new(),
            allocator: &bump,
            features: FeatureSet::DEFAULT,

            types: vec![],
            imports: vec![],
            functions,
            globals: Vec::new(),
            table: None,
            memory: None,
            start: None,
            exports: Vec::new(),
            elements: Vec::new(),
            data: Vec::new(),
            annotations: AnnotationStore::new(),
        };

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();

        assert!(!valid, "Validation should fail for mismatched types");
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Binary op AddInt32 operands type mismatch"));
    }

    #[test]
    fn test_global_validation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Helper to create standard globals
        let create_globals = || {
            vec![
                crate::ir::module::Global {
                    name: "g0".to_string(),
                    type_: Type::I32,
                    mutable: false,
                    init: builder.const_(Literal::I32(0)),
                },
                crate::ir::module::Global {
                    name: "g1".to_string(),
                    type_: Type::F32,
                    mutable: true,
                    init: builder.const_(Literal::F32(0.0)),
                },
            ]
        };

        // 1. Test GlobalSet on immutable global (g0)
        {
            let val = builder.const_(Literal::I32(42));
            let set_immutable = builder.global_set(0, val);

            let func = Function {
                name: "fail_immut".to_string(),
                type_idx: None,
                params: Type::NONE,
                results: Type::NONE,
                vars: vec![],
                body: Some(set_immutable),
                local_names: vec![],
            };

            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![func],
                globals: create_globals(),
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Setting immutable global should fail");
            assert!(errors.iter().any(|e| e.contains("is immutable")));
        }

        // 2. Test GlobalSet type mismatch
        {
            let val = builder.const_(Literal::I32(42)); // i32
            let set_mismatch = builder.global_set(1, val); // trying to set to g1 (f32)

            let func = Function {
                name: "fail_type".to_string(),
                type_idx: None,
                params: Type::NONE,
                results: Type::NONE,
                vars: vec![],
                body: Some(set_mismatch),
                local_names: vec![],
            };

            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![func],
                globals: create_globals(),
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "GlobalSet type mismatch should fail");
            assert!(errors
                .iter()
                .any(|e| e.contains("Value type i32 does not match global type f32")));
        }

        // 3. Test GlobalGet out of bounds
        {
            let get_oob = builder.global_get(99, Type::I32);

            let func = Function {
                name: "fail_oob".to_string(),
                type_idx: None,
                params: Type::NONE,
                results: Type::I32,
                vars: vec![],
                body: Some(get_oob),
                local_names: vec![],
            };

            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![func],
                globals: create_globals(),
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "OOB GlobalGet should fail");
            assert!(errors.iter().any(|e| e.contains("out of bounds")));
        }
    }

    #[test]
    fn test_ir_construction() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let const_expr = builder.const_(Literal::I32(42));

        match const_expr.kind {
            ExpressionKind::Const(Literal::I32(42)) => (),
            _ => panic!("Expected Const(42)"),
        }
        assert_eq!(const_expr.type_, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(const_expr);

        let block = builder.block(Some("my_block"), list, Type::I32);

        if let ExpressionKind::Block { name, list } = &block.kind {
            assert_eq!(*name, Some("my_block"));
            assert_eq!(list.len(), 1);
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_binary_op() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let left = builder.const_(Literal::I32(10));
        let right = builder.const_(Literal::I32(32));

        let add = builder.binary(BinaryOp::AddInt32, left, right, Type::I32);

        if let ExpressionKind::Binary { op, left, right } = &add.kind {
            assert_eq!(*op, BinaryOp::AddInt32);
            assert_eq!(left.type_, Type::I32);
            assert_eq!(right.type_, Type::I32);
        } else {
            panic!("Expected Binary");
        }
    }

    struct CountVisitor {
        count: usize,
    }

    impl<'a> Visitor<'a> for CountVisitor {
        fn visit_expression(&mut self, _expr: &mut ExprRef<'a>) {
            self.count += 1;
        }
    }

    #[test]
    fn test_visitor() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(add);
        let mut block = builder.block(None, list, Type::I32);

        let mut v = CountVisitor { count: 0 };
        v.visit(&mut block);

        // Block (1) -> Add (1) -> Const (1) + Const (1) = 4 expressions
        assert_eq!(v.count, 4);
    }

    #[test]
    fn test_module_construction() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // fn add_one(x: i32) -> i32
        let local_get = builder.local_get(0, Type::I32);
        let const_1 = builder.const_(Literal::I32(1));
        let add = builder.binary(BinaryOp::AddInt32, local_get, const_1, Type::I32);

        let func = Function {
            name: "add_one".to_string(),
            type_idx: None,
            params: Type::I32,
            results: Type::I32,
            vars: vec![],
            body: Some(add),
            local_names: vec![],
        };

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        assert!(module.get_function("add_one").is_some());

        let f = module.get_function("add_one").unwrap();
        if let Some(body) = &f.body {
            if let ExpressionKind::Binary { op, .. } = body.kind {
                assert_eq!(op, BinaryOp::AddInt32);
            } else {
                panic!("Expected Binary");
            }
        }
    }

    #[test]
    fn test_export_validation() {
        use crate::ir::module::{Export, ExportKind, Global, MemoryLimits};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Setup a basic valid module components
        let _func = Function {
            name: "f0".to_string(),
            type_idx: None,
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![],
            body: None,
            local_names: vec![],
        };

        let global_init = builder.const_(Literal::I32(0));
        let global = Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init: global_init,
        };

        let memory = MemoryLimits {
            initial: 1,
            maximum: None,
        };

        // 1. Valid exports
        {
            let func_valid = Function {
                name: "f0".to_string(),
                type_idx: None,
                params: Type::NONE,
                results: Type::NONE,
                vars: vec![],
                body: None,
                local_names: vec![],
            };

            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![func_valid],  // f0 is index 0
                globals: vec![global],        // g0 is index 0
                memory: Some(memory.clone()), // memory is index 0
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![
                    Export {
                        name: "exp_func".to_string(),
                        kind: ExportKind::Function,
                        index: 0,
                    },
                    Export {
                        name: "exp_glob".to_string(),
                        kind: ExportKind::Global,
                        index: 0,
                    },
                    Export {
                        name: "exp_mem".to_string(),
                        kind: ExportKind::Memory,
                        index: 0,
                    },
                ],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Valid exports failed: {:?}", errors);
        }

        // 2. Duplicate export name
        {
            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![],
                globals: vec![],
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![
                    Export {
                        name: "same".to_string(),
                        kind: ExportKind::Function,
                        index: 0,
                    },
                    Export {
                        name: "same".to_string(),
                        kind: ExportKind::Function,
                        index: 0,
                    },
                ],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };
            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid);
            assert!(errors.iter().any(|e| e.contains("Duplicate export name")));
        }

        // 3. Function OOB
        {
            let func_valid = Function {
                name: "f0".to_string(),
                type_idx: None,
                params: Type::NONE,
                results: Type::NONE,
                vars: vec![],
                body: None,
                local_names: vec![],
            };
            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![func_valid],
                globals: vec![],
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![Export {
                    name: "f1".to_string(),
                    kind: ExportKind::Function,
                    index: 1,
                }],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };
            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid);
            assert!(errors
                .iter()
                .any(|e| e.contains("Exported function index 1 out of bounds")));
        }

        // 4. Global OOB
        {
            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![],
                globals: vec![],
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![Export {
                    name: "g0".to_string(),
                    kind: ExportKind::Global,
                    index: 0,
                }],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };
            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid);
            assert!(errors
                .iter()
                .any(|e| e.contains("Exported global index 0 out of bounds")));
        }

        // 5. Memory OOB / No Memory
        {
            let module = Module {
                type_names: Vec::new(),
                allocator: &bump,
                features: FeatureSet::DEFAULT,

                types: vec![],
                imports: vec![],
                functions: vec![],
                globals: vec![],
                memory: None,
                start: None,
                table: None,
                elements: Vec::new(),
                exports: vec![Export {
                    name: "m0".to_string(),
                    kind: ExportKind::Memory,
                    index: 0,
                }],
                data: Vec::new(),
                annotations: AnnotationStore::new(),
            };
            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid);
            assert!(errors
                .iter()
                .any(|e| e.contains("Exported memory but no memory exists")));
        }
    }

    #[test]
    fn test_exports_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Export, ExportKind, Global, MemoryLimits};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], None);
        let global = Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init: builder.const_(Literal::I32(123)),
        };
        let memory = MemoryLimits {
            initial: 1,
            maximum: None,
        };

        let module = Module {
            type_names: Vec::new(),
            allocator: &bump,
            features: FeatureSet::DEFAULT,

            types: vec![],
            imports: vec![],
            functions: vec![func],
            globals: vec![global],
            memory: Some(memory),
            start: None,
            table: None,
            elements: Vec::new(),
            exports: vec![
                Export {
                    name: "test_func".to_string(),
                    kind: ExportKind::Function,
                    index: 0,
                },
                Export {
                    name: "test_glob".to_string(),
                    kind: ExportKind::Global,
                    index: 0,
                },
                Export {
                    name: "test_mem".to_string(),
                    kind: ExportKind::Memory,
                    index: 0,
                },
            ],
            data: Vec::new(),
            annotations: AnnotationStore::new(),
        };

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("write failed");

        // Read
        let read_bump = Bump::new();
        let mut reader = BinaryReader::new(&read_bump, bytes);
        let read_module = reader.parse_module().expect("parse failed");

        // Verify
        assert_eq!(read_module.exports.len(), 3);

        let func_exp = read_module
            .exports
            .iter()
            .find(|e| e.name == "test_func")
            .unwrap();
        assert_eq!(func_exp.kind, ExportKind::Function);
        assert_eq!(func_exp.index, 0);

        let glob_exp = read_module
            .exports
            .iter()
            .find(|e| e.name == "test_glob")
            .unwrap();
        assert_eq!(glob_exp.kind, ExportKind::Global);
        assert_eq!(glob_exp.index, 0);

        let mem_exp = read_module
            .exports
            .iter()
            .find(|e| e.name == "test_mem")
            .unwrap();
        assert_eq!(mem_exp.kind, ExportKind::Memory);
        assert_eq!(mem_exp.index, 0);
    }

    #[test]
    fn test_imports_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind, MemoryLimits};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // 1. Function Import (param: i32, result: none)
        module.add_import(Import {
            module: "env".to_string(),
            name: "log".to_string(),
            kind: ImportKind::Function(Type::I32, Type::NONE),
        });

        // 2. Global Import (i32, immutable)
        module.add_import(Import {
            module: "env".to_string(),
            name: "limit".to_string(),
            kind: ImportKind::Global(Type::I32, false),
        });

        // 3. Memory Import
        module.add_import(Import {
            module: "env".to_string(),
            name: "memory".to_string(),
            kind: ImportKind::Memory(MemoryLimits {
                initial: 1,
                maximum: Some(2),
            }),
        });

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("write failed");

        // Read
        let read_bump = Bump::new();
        let mut reader = BinaryReader::new(&read_bump, bytes);
        let read_module = reader.parse_module().expect("parse failed");

        assert_eq!(read_module.imports.len(), 3);

        let log_import = &read_module.imports[0];
        assert_eq!(log_import.module, "env");
        assert_eq!(log_import.name, "log");
        if let ImportKind::Function(p, r) = log_import.kind {
            assert_eq!(p, Type::I32);
            assert_eq!(r, Type::NONE);
        } else {
            panic!("Expected function import");
        }

        let global_import = &read_module.imports[1];
        assert_eq!(global_import.name, "limit");
        if let ImportKind::Global(ty, mut_) = global_import.kind {
            assert_eq!(ty, Type::I32);
            assert!(!mut_);
        } else {
            panic!("Expected global import");
        }
    }

    #[test]
    fn test_import_validation() {
        use crate::ir::module::{Global, Import, ImportKind};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Import Global 0: i32, immutable
        module.add_import(Import {
            module: "env".to_string(),
            name: "g_imp".to_string(),
            kind: ImportKind::Global(Type::I32, false),
        });

        // Define Global 1: f32, mutable
        let init_expr = builder.const_(Literal::F32(1.0));
        module.add_global(Global {
            name: "g_def".to_string(),
            type_: Type::F32,
            mutable: true,
            init: init_expr,
        });

        // Test 1: Get imported global (index 0) - Valid
        {
            let get_imp = builder.global_get(0, Type::I32);
            let func = Function::new(
                "test1".to_string(),
                Type::NONE,
                Type::I32,
                vec![],
                Some(get_imp),
            );
            module.functions.push(func);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Get imported global failed: {:?}", errors);
            module.functions.pop();
        }

        // Test 2: Get defined global (index 1) - Valid
        {
            let get_def = builder.global_get(1, Type::F32);
            let func = Function::new(
                "test2".to_string(),
                Type::NONE,
                Type::F32,
                vec![],
                Some(get_def),
            );
            module.functions.push(func);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Get defined global failed: {:?}", errors);
            module.functions.pop();
        }

        // Test 3: Get OOB global (index 2) - Invalid
        {
            let get_oob = builder.global_get(2, Type::I32);
            let func = Function::new(
                "test3".to_string(),
                Type::NONE,
                Type::I32,
                vec![],
                Some(get_oob),
            );
            module.functions.push(func);

            let validator = Validator::new(&module);
            let (valid, _) = validator.validate();
            assert!(!valid, "OOB global get should fail");
            module.functions.pop();
        }
    }

    #[test]
    fn test_data_section_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add memory
        module.set_memory(1, Some(10));

        // Data segment 1: "Hello" at offset 0
        let offset1 = builder.const_(Literal::I32(0));
        module.add_data_segment(DataSegment {
            memory_index: 0,
            offset: offset1,
            data: b"Hello".to_vec(),
        });

        // Data segment 2: "World" at offset 100
        let offset2 = builder.const_(Literal::I32(100));
        module.add_data_segment(DataSegment {
            memory_index: 0,
            offset: offset2,
            data: b"World".to_vec(),
        });

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("write failed");

        // Read
        let read_bump = Bump::new();
        let mut reader = BinaryReader::new(&read_bump, bytes);
        let read_module = reader.parse_module().expect("parse failed");

        // Verify
        assert_eq!(read_module.data.len(), 2);

        let seg1 = &read_module.data[0];
        assert_eq!(seg1.memory_index, 0);
        assert_eq!(seg1.data, b"Hello");
        if let ExpressionKind::Const(Literal::I32(val)) = seg1.offset.kind {
            assert_eq!(val, 0);
        } else {
            panic!("Expected const offset");
        }

        let seg2 = &read_module.data[1];
        assert_eq!(seg2.memory_index, 0);
        assert_eq!(seg2.data, b"World");
        if let ExpressionKind::Const(Literal::I32(val)) = seg2.offset.kind {
            assert_eq!(val, 100);
        } else {
            panic!("Expected const offset");
        }
    }

    #[test]
    fn test_data_section_validation() {
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test 1: Valid data segment
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.set_memory(1, None);

            let offset = builder.const_(Literal::I32(0));
            module.add_data_segment(DataSegment {
                memory_index: 0,
                offset,
                data: b"test".to_vec(),
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Valid data segment failed: {:?}", errors);
        }

        // Test 2: Data segment without memory
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            // No memory defined

            let offset = builder.const_(Literal::I32(0));
            module.add_data_segment(DataSegment {
                memory_index: 0,
                offset,
                data: b"test".to_vec(),
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Data without memory should fail");
            assert!(errors.iter().any(|e| e.contains("no memory exists")));
        }

        // Test 3: Invalid memory index
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.set_memory(1, None);

            let offset = builder.const_(Literal::I32(0));
            module.add_data_segment(DataSegment {
                memory_index: 1, // Invalid: only 0 allowed
                offset,
                data: b"test".to_vec(),
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Invalid memory index should fail");
            assert!(errors.iter().any(|e| e.contains("invalid memory index")));
        }
    }

    #[test]
    fn test_start_section_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a function to be the start function
        let body = builder.const_(Literal::I32(42));
        let func = Function::new(
            "init".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        );
        module.add_function(func);

        // Set as start function (index 0)
        module.set_start(0);

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("write failed");

        // Read
        let read_bump = Bump::new();
        let mut reader = BinaryReader::new(&read_bump, bytes);
        let read_module = reader.parse_module().expect("parse failed");

        // Verify
        assert_eq!(read_module.start, Some(0));
    }

    #[test]
    fn test_start_section_validation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test 1: Valid start function (no params, no results)
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let const_expr = builder.const_(Literal::I32(0));
            let body = builder.drop(const_expr); // Drop the i32 to get Type::NONE
            let func = Function::new(
                "start".to_string(),
                Type::NONE,
                Type::NONE,
                vec![],
                Some(body),
            );
            module.add_function(func);
            module.set_start(0);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Valid start function failed: {:?}", errors);
        }

        // Test 2: Start function out of bounds
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.set_start(99); // No functions exist

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "OOB start function should fail");
            assert!(errors.iter().any(|e| e.contains("out of bounds")));
        }

        // Test 3: Start function with parameters (invalid)
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let body = builder.local_get(0, Type::I32);
            let func = Function::new(
                "bad_start".to_string(),
                Type::I32, // Has parameter
                Type::NONE,
                vec![],
                Some(body),
            );
            module.add_function(func);
            module.set_start(0);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Start with params should fail");
            assert!(errors.iter().any(|e| e.contains("no parameters")));
        }

        // Test 4: Start function with results (invalid)
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let body = builder.const_(Literal::I32(42));
            let func = Function::new(
                "bad_start".to_string(),
                Type::NONE,
                Type::I32, // Has result
                vec![],
                Some(body),
            );
            module.add_function(func);
            module.set_start(0);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Start with results should fail");
            assert!(errors.iter().any(|e| e.contains("no results")));
        }
    }

    #[test]
    fn test_table_and_element_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::ElementSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add functions that will be referenced in the element segment
        let func1 = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], None);
        let func2 = Function::new("f1".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func1);
        module.add_function(func2);

        // Add table
        module.set_table(Type::FUNCREF, 10, Some(20));

        // Add element segment: initialize table[0..2] with functions [0, 1]
        let offset = builder.const_(Literal::I32(0));
        module.add_element_segment(ElementSegment {
            table_index: 0,
            offset,
            func_indices: vec![0, 1],
        });

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("write failed");

        // Read
        let read_bump = Bump::new();
        let mut reader = BinaryReader::new(&read_bump, bytes);
        let read_module = reader.parse_module().expect("parse failed");

        // Verify table
        assert!(read_module.table.is_some());
        let table = read_module.table.as_ref().unwrap();
        assert_eq!(table.element_type, Type::FUNCREF);
        assert_eq!(table.initial, 10);
        assert_eq!(table.maximum, Some(20));

        // Verify element segments
        assert_eq!(read_module.elements.len(), 1);
        let elem = &read_module.elements[0];
        assert_eq!(elem.table_index, 0);
        assert_eq!(elem.func_indices, vec![0, 1]);
        if let ExpressionKind::Const(Literal::I32(val)) = elem.offset.kind {
            assert_eq!(val, 0);
        } else {
            panic!("Expected const offset");
        }
    }

    #[test]
    fn test_table_and_element_validation() {
        use crate::ir::module::ElementSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test 1: Valid table and element segment
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], None);
            module.add_function(func);

            module.set_table(Type::FUNCREF, 5, None);

            let offset = builder.const_(Literal::I32(0));
            module.add_element_segment(ElementSegment {
                table_index: 0,
                offset,
                func_indices: vec![0],
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Valid table/element failed: {:?}", errors);
        }

        // Test 2: Element segment without table
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], None);
            module.add_function(func);

            let offset = builder.const_(Literal::I32(0));
            module.add_element_segment(ElementSegment {
                table_index: 0,
                offset,
                func_indices: vec![0],
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Element without table should fail");
            assert!(errors.iter().any(|e| e.contains("no table exists")));
        }

        // Test 3: Element with invalid function index
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            module.set_table(Type::FUNCREF, 5, None);

            let offset = builder.const_(Literal::I32(0));
            module.add_element_segment(ElementSegment {
                table_index: 0,
                offset,
                func_indices: vec![99], // Out of bounds
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Element with OOB function should fail");
            assert!(errors.iter().any(|e| e.contains("out of bounds")));
        }

        // Test 4: Invalid table index in element segment
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], None);
            module.add_function(func);

            module.set_table(Type::FUNCREF, 5, None);

            let offset = builder.const_(Literal::I32(0));
            module.add_element_segment(ElementSegment {
                table_index: 1, // Invalid: only 0 allowed
                offset,
                func_indices: vec![0],
            });

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Invalid table index should fail");
            assert!(errors.iter().any(|e| e.contains("invalid table index")));
        }
    }

    #[test]
    fn test_type_section_roundtrip() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test 1: Empty types
        {
            let bump = bumpalo::Bump::new();
            let module = Module::new(&bump);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 0);
        }

        // Test 2: Single type (i32) -> (i32)
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::I32, Type::I32);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 1);
            assert_eq!(parsed.types[0].params, Type::I32);
            assert_eq!(parsed.types[0].results, Type::I32);
        }

        // Test 3: Multiple types
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::I32, Type::I32);
            module.add_type(Type::I32, Type::NONE);
            module.add_type(Type::NONE, Type::F64);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 3);
            assert_eq!(parsed.types[0].params, Type::I32);
            assert_eq!(parsed.types[0].results, Type::I32);
            assert_eq!(parsed.types[1].params, Type::I32);
            assert_eq!(parsed.types[1].results, Type::NONE);
            assert_eq!(parsed.types[2].params, Type::NONE);
            assert_eq!(parsed.types[2].results, Type::F64);
        }
    }

    #[test]
    fn test_type_section_all_basic_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test all basic WebAssembly value types
        let test_types = [
            (Type::I32, "i32"),
            (Type::I64, "i64"),
            (Type::F32, "f32"),
            (Type::F64, "f64"),
            (Type::V128, "v128"),
        ];

        for (param_type, param_name) in &test_types {
            for (result_type, result_name) in &test_types {
                let bump = bumpalo::Bump::new();
                let mut module = Module::new(&bump);
                module.add_type(*param_type, *result_type);

                let mut writer = BinaryWriter::new();
                let bytes = writer.write_module(&module).unwrap_or_else(|_| {
                    panic!("Failed to write {} -> {}", param_name, result_name)
                });

                let mut reader = BinaryReader::new(&bump, bytes);
                let parsed = reader.parse_module().unwrap_or_else(|_| {
                    panic!("Failed to parse {} -> {}", param_name, result_name)
                });

                assert_eq!(
                    parsed.types.len(),
                    1,
                    "Wrong type count for {} -> {}",
                    param_name,
                    result_name
                );
                assert_eq!(
                    parsed.types[0].params, *param_type,
                    "Wrong param type for {} -> {}",
                    param_name, result_name
                );
                assert_eq!(
                    parsed.types[0].results, *result_type,
                    "Wrong result type for {} -> {}",
                    param_name, result_name
                );
            }
        }
    }

    #[test]
    fn test_type_section_empty_signatures() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test 1: () -> ()
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::NONE, Type::NONE);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 1);
            assert_eq!(parsed.types[0].params, Type::NONE);
            assert_eq!(parsed.types[0].results, Type::NONE);
        }

        // Test 2: () -> (i32)
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::NONE, Type::I32);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 1);
            assert_eq!(parsed.types[0].params, Type::NONE);
            assert_eq!(parsed.types[0].results, Type::I32);
        }

        // Test 3: (f64) -> ()
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::F64, Type::NONE);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.types.len(), 1);
            assert_eq!(parsed.types[0].params, Type::F64);
            assert_eq!(parsed.types[0].results, Type::NONE);
        }
    }

    #[test]
    fn test_type_section_reference_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test funcref and externref
        let ref_types = [(Type::FUNCREF, "funcref"), (Type::EXTERNREF, "externref")];

        for (ref_type, name) in &ref_types {
            // Test: (ref) -> ()
            {
                let bump = bumpalo::Bump::new();
                let mut module = Module::new(&bump);
                module.add_type(*ref_type, Type::NONE);

                let mut writer = BinaryWriter::new();
                let bytes = writer
                    .write_module(&module)
                    .unwrap_or_else(|_| panic!("Failed to write {} -> ()", name));

                let mut reader = BinaryReader::new(&bump, bytes);
                let parsed = reader
                    .parse_module()
                    .unwrap_or_else(|_| panic!("Failed to parse {} -> ()", name));

                assert_eq!(parsed.types.len(), 1);
                assert_eq!(parsed.types[0].params, *ref_type);
                assert_eq!(parsed.types[0].results, Type::NONE);
            }

            // Test: () -> (ref)
            {
                let bump = bumpalo::Bump::new();
                let mut module = Module::new(&bump);
                module.add_type(Type::NONE, *ref_type);

                let mut writer = BinaryWriter::new();
                let bytes = writer
                    .write_module(&module)
                    .unwrap_or_else(|_| panic!("Failed to write () -> {}", name));

                let mut reader = BinaryReader::new(&bump, bytes);
                let parsed = reader
                    .parse_module()
                    .unwrap_or_else(|_| panic!("Failed to parse () -> {}", name));

                assert_eq!(parsed.types.len(), 1);
                assert_eq!(parsed.types[0].params, Type::NONE);
                assert_eq!(parsed.types[0].results, *ref_type);
            }
        }
    }

    #[test]
    fn test_type_section_many_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test with 100 types to ensure proper LEB128 encoding and no limit issues
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        for i in 0..100 {
            let param_type = match i % 5 {
                0 => Type::I32,
                1 => Type::I64,
                2 => Type::F32,
                3 => Type::F64,
                _ => Type::NONE,
            };
            let result_type = match i % 4 {
                0 => Type::I32,
                1 => Type::F64,
                2 => Type::NONE,
                _ => Type::I64,
            };
            module.add_type(param_type, result_type);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 100, "Should have 100 types");

        // Verify each type matches
        for i in 0..100 {
            let expected_param = match i % 5 {
                0 => Type::I32,
                1 => Type::I64,
                2 => Type::F32,
                3 => Type::F64,
                _ => Type::NONE,
            };
            let expected_result = match i % 4 {
                0 => Type::I32,
                1 => Type::F64,
                2 => Type::NONE,
                _ => Type::I64,
            };
            assert_eq!(
                parsed.types[i].params, expected_param,
                "Type {} param mismatch",
                i
            );
            assert_eq!(
                parsed.types[i].results, expected_result,
                "Type {} result mismatch",
                i
            );
        }
    }

    #[test]
    fn test_type_section_with_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test that types are properly associated with functions
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add explicit types
        module.add_type(Type::I32, Type::I32); // type 0
        module.add_type(Type::F64, Type::F64); // type 1

        // Add functions that should reference these types
        let body1 = builder.const_(Literal::I32(42));
        let func1 = Function::new("f1".to_string(), Type::I32, Type::I32, vec![], Some(body1));
        module.add_function(func1);

        let body2 = builder.const_(Literal::F64(1.23));
        let func2 = Function::new("f2".to_string(), Type::F64, Type::F64, vec![], Some(body2));
        module.add_function(func2);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Should have at least the 2 explicit types (writer may add more if needed)
        assert!(parsed.types.len() >= 2, "Should have at least 2 types");

        // Functions should be parsed
        assert_eq!(parsed.functions.len(), 2, "Should have 2 functions");
    }

    #[test]
    fn test_type_section_with_imports() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add explicit types
        module.add_type(Type::I32, Type::I32); // type 0
        module.add_type(Type::F32, Type::F32); // type 1

        // Add imports that use these types
        module.add_import(Import {
            module: "env".to_string(),
            name: "add".to_string(),
            kind: ImportKind::Function(Type::I32, Type::I32),
        });

        module.add_import(Import {
            module: "env".to_string(),
            name: "sqrt".to_string(),
            kind: ImportKind::Function(Type::F32, Type::F32),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Should have the explicit types
        assert_eq!(parsed.types.len(), 2, "Should have 2 types");

        // Imports should be parsed
        assert_eq!(parsed.imports.len(), 2, "Should have 2 imports");
    }

    #[test]
    fn test_type_section_deduplication() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test that writer deduplicates identical type signatures
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add three functions with the same signature
        for i in 0..3 {
            let body = builder.const_(Literal::I32(i));
            let func = Function::new(format!("f{}", i), Type::I32, Type::I32, vec![], Some(body));
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Should only have 1 type (deduplicated)
        assert_eq!(
            parsed.types.len(),
            1,
            "Should deduplicate identical signatures"
        );
        assert_eq!(parsed.types[0].params, Type::I32);
        assert_eq!(parsed.types[0].results, Type::I32);

        // Should still have 3 functions
        assert_eq!(parsed.functions.len(), 3);
    }

    #[test]
    fn test_function_section_explicit_type_indices() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test explicit type_idx in functions
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add explicit types
        module.add_type(Type::I32, Type::I32); // type 0
        module.add_type(Type::F64, Type::F64); // type 1
        module.add_type(Type::NONE, Type::I64); // type 2

        // Create functions with explicit type indices
        let body1 = builder.const_(Literal::I32(42));
        let func1 = Function::with_type_idx(
            "f0".to_string(),
            0,
            Type::I32,
            Type::I32,
            vec![],
            Some(body1),
        );
        module.add_function(func1);

        let body2 = builder.const_(Literal::F64(std::f64::consts::PI));
        let func2 = Function::with_type_idx(
            "f1".to_string(),
            1,
            Type::F64,
            Type::F64,
            vec![],
            Some(body2),
        );
        module.add_function(func2);

        let body3 = builder.const_(Literal::I64(99));
        let func3 = Function::with_type_idx(
            "f2".to_string(),
            2,
            Type::NONE,
            Type::I64,
            vec![],
            Some(body3),
        );
        module.add_function(func3);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 3);
        assert_eq!(parsed.functions.len(), 3);

        // Verify type indices are preserved
        assert_eq!(parsed.functions[0].type_idx, Some(0));
        assert_eq!(parsed.functions[1].type_idx, Some(1));
        assert_eq!(parsed.functions[2].type_idx, Some(2));
    }

    #[test]
    fn test_function_section_mixed_type_specification() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test mixing explicit type_idx and inferred types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_type(Type::I32, Type::I32); // type 0

        // Function with explicit type_idx
        let body1 = builder.const_(Literal::I32(1));
        let func1 = Function::with_type_idx(
            "explicit".to_string(),
            0,
            Type::I32,
            Type::I32,
            vec![],
            Some(body1),
        );
        module.add_function(func1);

        // Function without type_idx (will be inferred)
        let body2 = builder.const_(Literal::F64(2.0));
        let func2 = Function::new(
            "inferred".to_string(),
            Type::F64,
            Type::F64,
            vec![],
            Some(body2),
        );
        module.add_function(func2);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
        // Both functions should have type_idx after round-trip
        assert!(parsed.functions[0].type_idx.is_some());
        assert!(parsed.functions[1].type_idx.is_some());
    }

    #[test]
    fn test_function_section_many_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test with 50 functions
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        for i in 0..50 {
            let (param_type, result_type) = match i % 4 {
                0 => (Type::I32, Type::I32),
                1 => (Type::F64, Type::F64),
                2 => (Type::NONE, Type::I32),
                _ => (Type::I64, Type::NONE),
            };

            let body = match result_type {
                t if t == Type::I32 => builder.const_(Literal::I32(i)),
                t if t == Type::I64 => builder.const_(Literal::I64(i as i64)),
                t if t == Type::F64 => builder.const_(Literal::F64(i as f64)),
                _ => builder.nop(),
            };

            let func = Function::new(
                format!("func_{}", i),
                param_type,
                result_type,
                vec![],
                Some(body),
            );
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 50, "Should have 50 functions");
        // All functions should have type indices
        for (i, func) in parsed.functions.iter().enumerate() {
            assert!(
                func.type_idx.is_some(),
                "Function {} should have type_idx",
                i
            );
        }
    }

    #[test]
    fn test_function_section_validation_type_idx_bounds() {
        use crate::validation::Validator;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test 1: Valid type_idx
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::I32, Type::I32);

            let body = builder.const_(Literal::I32(42));
            let func = Function::with_type_idx(
                "valid".to_string(),
                0,
                Type::I32,
                Type::I32,
                vec![],
                Some(body),
            );
            module.add_function(func);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(valid, "Valid type_idx should pass: {:?}", errors);
        }

        // Test 2: Out of bounds type_idx
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::I32, Type::I32);

            let body = builder.const_(Literal::I32(42));
            let func = Function::with_type_idx(
                "oob".to_string(),
                99, // Out of bounds
                Type::I32,
                Type::I32,
                vec![],
                Some(body),
            );
            module.add_function(func);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Out of bounds type_idx should fail");
            assert!(errors.iter().any(|e| e.contains("out of bounds")));
        }

        // Test 3: Type signature mismatch
        {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);
            module.add_type(Type::I32, Type::I32);

            let body = builder.const_(Literal::F64(std::f64::consts::PI));
            let func = Function::with_type_idx(
                "mismatch".to_string(),
                0,
                Type::F64, // Doesn't match type 0
                Type::F64,
                vec![],
                Some(body),
            );
            module.add_function(func);

            let validator = Validator::new(&module);
            let (valid, errors) = validator.validate();
            assert!(!valid, "Signature mismatch should fail");
            assert!(errors.iter().any(|e| e.contains("Signature mismatch")));
        }
    }

    #[test]
    fn test_function_section_no_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test that functions work even without explicit types in module
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.const_(Literal::I32(42));
        let func = Function::new("f".to_string(), Type::I32, Type::I32, vec![], Some(body));
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Writer should have inferred and created a type
        assert!(!parsed.types.is_empty(), "Should have inferred types");
        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_function_section_all_value_type_combinations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let value_types = [
            (Type::I32, Literal::I32(0)),
            (Type::I64, Literal::I64(0)),
            (Type::F32, Literal::F32(0.0)),
            (Type::F64, Literal::F64(0.0)),
        ];

        for (i, (param_type, _)) in value_types.iter().enumerate() {
            for (j, (result_type, lit)) in value_types.iter().enumerate() {
                let bump = bumpalo::Bump::new();
                let mut module = Module::new(&bump);

                let body = builder.const_(lit.clone());
                let func = Function::new(
                    format!("f_{}_{}", i, j),
                    *param_type,
                    *result_type,
                    vec![],
                    Some(body),
                );
                module.add_function(func);

                let mut writer = BinaryWriter::new();
                let bytes = writer.write_module(&module).unwrap_or_else(|e| {
                    panic!("Failed to write param {} result {}: {:?}", i, j, e)
                });

                let mut reader = BinaryReader::new(&bump, bytes);
                let parsed = reader.parse_module().unwrap_or_else(|e| {
                    panic!("Failed to parse param {} result {}: {:?}", i, j, e)
                });

                assert_eq!(parsed.functions.len(), 1);
                assert_eq!(parsed.functions[0].params, *param_type);
                assert_eq!(parsed.functions[0].results, *result_type);
            }
        }
    }

    #[test]
    fn test_function_section_empty() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test module with types but no functions
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_type(Type::I32, Type::I32);
        module.add_type(Type::F64, Type::F64);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 2);
        assert_eq!(parsed.functions.len(), 0);
    }

    #[test]
    fn test_function_section_with_locals() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test functions with various local variable counts
        let test_cases = [
            (vec![], "no locals"),
            (vec![Type::I32], "1 i32 local"),
            (vec![Type::I32, Type::I64], "2 locals"),
            (
                vec![Type::I32, Type::I64, Type::F32, Type::F64],
                "4 different locals",
            ),
            (vec![Type::I32; 10], "10 i32 locals"),
        ];

        for (locals, desc) in &test_cases {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let body = builder.const_(Literal::I32(42));
            let func = Function::new(
                format!("f_{}", desc),
                Type::NONE,
                Type::I32,
                locals.clone(),
                Some(body),
            );
            module.add_function(func);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|e| panic!("Failed to write: {}: {:?}", desc, e));

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader
                .parse_module()
                .unwrap_or_else(|e| panic!("Failed to parse: {}: {:?}", desc, e));

            assert_eq!(
                parsed.functions.len(),
                1,
                "Function count mismatch: {}",
                desc
            );
            assert_eq!(
                parsed.functions[0].vars.len(),
                locals.len(),
                "Local count mismatch: {}",
                desc
            );
        }
    }

    #[test]
    fn test_function_section_shared_type_indices() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test multiple functions sharing the same type
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_type(Type::I32, Type::I32); // type 0

        // Create 5 functions all using type 0
        for i in 0..5 {
            let body = builder.const_(Literal::I32(i));
            let func = Function::with_type_idx(
                format!("shared_{}", i),
                0,
                Type::I32,
                Type::I32,
                vec![],
                Some(body),
            );
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 1, "Should have 1 shared type");
        assert_eq!(parsed.functions.len(), 5);

        // All functions should reference type 0
        for (i, func) in parsed.functions.iter().enumerate() {
            assert_eq!(
                func.type_idx,
                Some(0),
                "Function {} should reference type 0",
                i
            );
        }
    }

    #[test]
    fn test_function_section_type_ordering() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test that type indices are correctly maintained in order
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add types in specific order
        module.add_type(Type::I32, Type::I32); // type 0
        module.add_type(Type::I64, Type::I64); // type 1
        module.add_type(Type::F32, Type::F32); // type 2
        module.add_type(Type::F64, Type::F64); // type 3

        // Add functions referencing types in reverse order
        for i in (0..4).rev() {
            let (typ, lit) = match i {
                0 => (Type::I32, Literal::I32(0)),
                1 => (Type::I64, Literal::I64(0)),
                2 => (Type::F32, Literal::F32(0.0)),
                _ => (Type::F64, Literal::F64(0.0)),
            };

            let body = builder.const_(lit);
            let func = Function::with_type_idx(format!("f{}", i), i, typ, typ, vec![], Some(body));
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 4);

        // Verify type indices are preserved (reverse order: 3, 2, 1, 0)
        assert_eq!(parsed.functions[0].type_idx, Some(3));
        assert_eq!(parsed.functions[1].type_idx, Some(2));
        assert_eq!(parsed.functions[2].type_idx, Some(1));
        assert_eq!(parsed.functions[3].type_idx, Some(0));
    }

    #[test]
    fn test_function_section_with_imports() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test interaction between imported and defined functions
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_type(Type::I32, Type::I32); // type 0
        module.add_type(Type::F64, Type::F64); // type 1

        // Add 2 imported functions
        module.add_import(Import {
            module: "env".to_string(),
            name: "imported1".to_string(),
            kind: ImportKind::Function(Type::I32, Type::I32),
        });

        module.add_import(Import {
            module: "env".to_string(),
            name: "imported2".to_string(),
            kind: ImportKind::Function(Type::F64, Type::F64),
        });

        // Add 2 defined functions
        let body1 = builder.const_(Literal::I32(1));
        let func1 = Function::with_type_idx(
            "local1".to_string(),
            0,
            Type::I32,
            Type::I32,
            vec![],
            Some(body1),
        );
        module.add_function(func1);

        let body2 = builder.const_(Literal::F64(2.0));
        let func2 = Function::with_type_idx(
            "local2".to_string(),
            1,
            Type::F64,
            Type::F64,
            vec![],
            Some(body2),
        );
        module.add_function(func2);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Function space: imports come first, then defined functions
        assert_eq!(parsed.imports.len(), 2);
        assert_eq!(parsed.functions.len(), 2);
        assert_eq!(parsed.types.len(), 2);
    }

    #[test]
    fn test_function_section_no_body() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test function with no body (None)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_type(Type::I32, Type::I32);

        let func =
            Function::with_type_idx("no_body".to_string(), 0, Type::I32, Type::I32, vec![], None);
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        // Body parsing might differ - just verify structure is preserved
    }

    #[test]
    fn test_function_section_reference_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test functions with reference types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // funcref parameter
        let body1 = builder.nop();
        let func1 = Function::new(
            "takes_funcref".to_string(),
            Type::FUNCREF,
            Type::NONE,
            vec![],
            Some(body1),
        );
        module.add_function(func1);

        // externref parameter
        let body2 = builder.nop();
        let func2 = Function::new(
            "takes_externref".to_string(),
            Type::EXTERNREF,
            Type::NONE,
            vec![],
            Some(body2),
        );
        module.add_function(func2);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
        assert_eq!(parsed.functions[0].params, Type::FUNCREF);
        assert_eq!(parsed.functions[1].params, Type::EXTERNREF);
    }

    #[test]
    fn test_function_section_type_inference_deduplication() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test that writer correctly deduplicates inferred types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add 10 functions with same signature (no explicit type_idx)
        for i in 0..10 {
            let body = builder.const_(Literal::I32(i));
            let func = Function::new(
                format!("inferred_{}", i),
                Type::I32,
                Type::I32,
                vec![],
                Some(body),
            );
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        // Should deduplicate to 1 type
        assert_eq!(
            parsed.types.len(),
            1,
            "Should deduplicate 10 identical inferred types to 1"
        );
        assert_eq!(parsed.functions.len(), 10);

        // All functions should reference the same type
        for (i, func) in parsed.functions.iter().enumerate() {
            assert_eq!(
                func.type_idx,
                Some(0),
                "Function {} should reference type 0",
                i
            );
        }
    }

    #[test]
    fn test_function_section_validation_empty_module() {
        use crate::validation::Validator;

        // Test validation with no functions
        let bump = bumpalo::Bump::new();
        let module = Module::new(&bump);
        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Empty module should be valid: {:?}", errors);
    }

    #[test]
    fn test_function_section_validation_no_type_idx() {
        use crate::validation::Validator;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test validation of function without explicit type_idx
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.const_(Literal::I32(42));
        let func = Function::new("f".to_string(), Type::I32, Type::I32, vec![], Some(body));
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(
            valid,
            "Function without type_idx should be valid: {:?}",
            errors
        );
    }

    #[test]
    fn test_function_section_mixed_signatures() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test with diverse function signatures
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let signatures = [
            (Type::NONE, Type::NONE, Literal::I32(0), "() -> ()"),
            (Type::I32, Type::I32, Literal::I32(1), "(i32) -> (i32)"),
            (Type::I64, Type::NONE, Literal::I32(2), "(i64) -> ()"),
            (Type::NONE, Type::F32, Literal::F32(3.0), "() -> (f32)"),
            (Type::F64, Type::I64, Literal::I64(4), "(f64) -> (i64)"),
        ];

        for (i, (param, result, lit, _desc)) in signatures.iter().enumerate() {
            let body = builder.const_(lit.clone());
            let func = Function::new(format!("f{}", i), *param, *result, vec![], Some(body));
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 5);

        // Verify each function has correct signature
        for (i, (param, result, _, _)) in signatures.iter().enumerate() {
            assert_eq!(parsed.functions[i].params, *param);
            assert_eq!(parsed.functions[i].results, *result);
        }
    }

    #[test]
    fn test_function_section_large_type_index() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test with many types to ensure large type indices work
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add 200 types
        for i in 0..200 {
            let typ = match i % 4 {
                0 => Type::I32,
                1 => Type::I64,
                2 => Type::F32,
                _ => Type::F64,
            };
            module.add_type(typ, typ);
        }

        // Add function referencing type 199
        let body = builder.const_(Literal::F64(std::f64::consts::PI));
        let func = Function::with_type_idx(
            "f199".to_string(),
            199,
            Type::F64,
            Type::F64,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 200);
        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].type_idx, Some(199));
    }

    #[test]
    fn test_function_section_locals_all_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test function with locals of all basic types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let locals = vec![Type::I32, Type::I64, Type::F32, Type::F64, Type::V128];

        let body = builder.const_(Literal::I32(0));
        let func = Function::new(
            "all_locals".to_string(),
            Type::NONE,
            Type::I32,
            locals,
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].vars.len(), 5);
        assert_eq!(parsed.functions[0].vars[0], Type::I32);
        assert_eq!(parsed.functions[0].vars[1], Type::I64);
        assert_eq!(parsed.functions[0].vars[2], Type::F32);
        assert_eq!(parsed.functions[0].vars[3], Type::F64);
        assert_eq!(parsed.functions[0].vars[4], Type::V128);
    }

    // ========== Code Section (Section 10) Tests ==========

    #[test]
    fn test_code_section_simple_const() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.const_(Literal::I32(42));
        let func = Function::new(
            "const_i32".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert!(parsed.functions[0].body.is_some());
    }

    #[test]
    fn test_code_section_all_const_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test each constant type
        let test_cases = [
            (Literal::I32(42), Type::I32),
            (Literal::I64(123456), Type::I64),
            (Literal::F32(std::f32::consts::PI), Type::F32),
            (Literal::F64(std::f64::consts::E), Type::F64),
        ];

        for (lit, typ) in &test_cases {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let body = builder.const_(lit.clone());
            let func = Function::new(
                "const_test".to_string(),
                Type::NONE,
                *typ,
                vec![],
                Some(body),
            );
            module.add_function(func);

            let mut writer = BinaryWriter::new();
            let bytes = writer.write_module(&module).expect("Failed to write");

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader.parse_module().expect("Failed to parse");

            assert_eq!(parsed.functions.len(), 1);
            assert_eq!(parsed.functions[0].results, *typ);
        }
    }

    #[test]
    fn test_code_section_binary_ops() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::ops::BinaryOp;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test only the integer binary operations that are currently supported
        let test_ops = [
            (BinaryOp::AddInt32, "add_i32"),
            (BinaryOp::SubInt32, "sub_i32"),
            (BinaryOp::MulInt32, "mul_i32"),
            (BinaryOp::AddInt64, "add_i64"),
        ];

        for (op, name) in &test_ops {
            let bump = bumpalo::Bump::new();
            let mut module = Module::new(&bump);

            let left = builder.const_(Literal::I32(10));
            let right = builder.const_(Literal::I32(20));
            let body = builder.binary(*op, left, right, Type::I32);

            let func = Function::new(
                format!("test_{}", name),
                Type::NONE,
                Type::I32,
                vec![],
                Some(body),
            );
            module.add_function(func);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|e| panic!("Failed to write {}: {:?}", name, e));

            let mut reader = BinaryReader::new(&bump, bytes);
            let parsed = reader
                .parse_module()
                .unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", name, e));

            assert_eq!(parsed.functions.len(), 1);
        }
    }

    // Note: Unary operations not yet supported in binary writer
    // #[test]
    // fn test_code_section_unary_ops() { ... }

    #[test]
    fn test_code_section_nested_expressions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::ops::BinaryOp;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test: (1 + 2) + (3 + 4)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let add1 = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        let c3 = builder.const_(Literal::I32(3));
        let c4 = builder.const_(Literal::I32(4));
        let add2 = builder.binary(BinaryOp::AddInt32, c3, c4, Type::I32);

        let result = builder.binary(BinaryOp::AddInt32, add1, add2, Type::I32);

        let func = Function::new(
            "nested".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(result),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert!(parsed.functions[0].body.is_some());
    }

    #[test]
    fn test_code_section_local_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test local.get and local.set
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function with 1 local
        let val = builder.const_(Literal::I32(42));
        let set_local = builder.local_set(0, val);
        let get_local = builder.local_get(0, Type::I32);
        let mut list = BumpVec::new_in(&bump);
        list.push(set_local);
        list.push(get_local);
        let body = builder.block(None, list, Type::I32);

        let func = Function::new(
            "locals".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].vars.len(), 1);
    }

    #[test]
    fn test_code_section_control_flow_block() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test block control flow
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let mut list = BumpVec::new_in(&bump);
        list.push(c1);
        list.push(c2);
        let body = builder.block(None, list, Type::I32);

        let func = Function::new(
            "block".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        if let Some(body) = &parsed.functions[0].body {
            if let ExpressionKind::Block { .. } = &body.kind {
                // Success - block parsed correctly
            } else {
                panic!("Expected Block expression");
            }
        }
    }

    // Note: If expressions not yet supported in binary writer
    // #[test]
    // fn test_code_section_control_flow_if() { ... }

    #[test]
    fn test_code_section_control_flow_loop() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test loop control flow
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let loop_body = builder.const_(Literal::I32(1));
        let body = builder.loop_(None, loop_body, Type::I32);

        let func = Function::new(
            "loop".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        if let Some(body) = &parsed.functions[0].body {
            if let ExpressionKind::Loop { .. } = &body.kind {
                // Success - loop parsed correctly
            } else {
                panic!("Expected Loop expression");
            }
        }
    }

    #[test]
    fn test_code_section_nop() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test nop instruction
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.nop();
        let func = Function::new(
            "nop".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        if let Some(body) = &parsed.functions[0].body {
            if let ExpressionKind::Nop = &body.kind {
                // Success - nop parsed correctly
            } else {
                panic!("Expected Nop expression");
            }
        }
    }

    #[test]
    fn test_code_section_multiple_functions_with_bodies() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::ops::BinaryOp;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test multiple functions with different body types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function 1: constant
        let body1 = builder.const_(Literal::I32(1));
        let func1 = Function::new("f1".to_string(), Type::NONE, Type::I32, vec![], Some(body1));
        module.add_function(func1);

        // Function 2: binary operation
        let c1 = builder.const_(Literal::I32(2));
        let c2 = builder.const_(Literal::I32(3));
        let body2 = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);
        let func2 = Function::new("f2".to_string(), Type::NONE, Type::I32, vec![], Some(body2));
        module.add_function(func2);

        // Function 3: nop
        let body3 = builder.nop();
        let func3 = Function::new(
            "f3".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body3),
        );
        module.add_function(func3);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 3);
        assert!(parsed.functions[0].body.is_some());
        assert!(parsed.functions[1].body.is_some());
        assert!(parsed.functions[2].body.is_some());
    }

    #[test]
    fn test_code_section_deep_nesting() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test deeply nested blocks (10 levels)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let mut expr = builder.const_(Literal::I32(42));
        for _ in 0..10 {
            let mut list = BumpVec::new_in(&bump);
            list.push(expr);
            expr = builder.block(None, list, Type::I32);
        }

        let func = Function::new(
            "deep".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(expr),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert!(parsed.functions[0].body.is_some());
    }

    #[test]
    fn test_code_section_global_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test global.get
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a global
        let global_init = builder.const_(Literal::I32(100));
        let global = Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init: global_init,
        };
        module.add_global(global);

        // Function that gets the global
        let body = builder.global_get(0, Type::I32);
        let func = Function::new(
            "get_global".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.globals.len(), 1);
    }

    #[test]
    fn test_code_section_empty_function_body() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test function with no body (None)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let func = Function::new("empty".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ========== Validation and Edge Case Tests ==========

    #[test]
    fn test_validation_type_index_out_of_bounds() {
        use crate::validation::Validator;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a function with invalid type_idx (no types defined)
        let func = Function {
            name: "bad_func".to_string(),
            params: Type::NONE,
            results: Type::I32,
            vars: vec![],
            body: None,
            type_idx: Some(99), // Invalid: no types exist
            local_names: vec![],
        };
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();

        assert!(
            !valid,
            "Should fail validation with out-of-bounds type index"
        );
        assert!(!errors.is_empty(), "Should have validation errors");
    }

    #[test]
    fn test_validation_type_signature_mismatch() {
        use crate::validation::Validator;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a type
        module.add_type(Type::I32, Type::I64);

        // Add function with mismatched signature
        let func = Function {
            name: "mismatch".to_string(),
            params: Type::I32,
            results: Type::F32, // Mismatch: type says I64
            vars: vec![],
            body: None,
            type_idx: Some(0),
            local_names: vec![String::new(); 1],
        };
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();

        assert!(!valid, "Should fail validation with signature mismatch");
        assert!(!errors.is_empty(), "Should have validation errors");
    }

    #[test]
    fn test_validation_valid_type_idx() {
        use crate::validation::Validator;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a type
        module.add_type(Type::I32, Type::I64);

        // Add function with valid type_idx and matching signature
        let func = Function {
            name: "valid".to_string(),
            params: Type::I32,
            results: Type::I64,
            vars: vec![],
            body: None,
            type_idx: Some(0),
            local_names: vec![String::new(); 1],
        };
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();

        assert!(valid, "Should pass validation. Errors: {:?}", errors);
    }

    #[test]
    fn test_roundtrip_maximum_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test with maximum reasonable number of types (1000)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        for i in 0..1000 {
            let params = if i % 2 == 0 { Type::I32 } else { Type::I64 };
            let results = if i % 3 == 0 { Type::F32 } else { Type::F64 };
            module.add_type(params, results);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 1000);
    }

    #[test]
    fn test_roundtrip_maximum_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test with many functions (500)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        for i in 0..500 {
            let func = Function::new(format!("func_{}", i), Type::NONE, Type::I32, vec![], None);
            module.add_function(func);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 500);
    }

    #[test]
    fn test_roundtrip_various_type_signatures() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test with various single-param single-result types
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // i32 -> i64
        module.add_type(Type::I32, Type::I64);

        // f32 -> f64
        module.add_type(Type::F32, Type::F64);

        // i64 -> i32
        module.add_type(Type::I64, Type::I32);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.types.len(), 3);
    }

    #[test]
    fn test_roundtrip_empty_module() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test completely empty module
        let bump = bumpalo::Bump::new();
        let module = Module::new(&bump);

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .expect("Failed to write empty module");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse empty module");

        assert_eq!(parsed.types.len(), 0);
        assert_eq!(parsed.functions.len(), 0);
        assert_eq!(parsed.globals.len(), 0);
        assert!(parsed.table.is_none());
    }

    #[test]
    fn test_roundtrip_mixed_function_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        // Test functions with and without type_idx
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a type
        module.add_type(Type::I32, Type::I64);

        // Function with explicit type_idx
        module.add_function(Function {
            name: "typed".to_string(),
            params: Type::I32,
            results: Type::I64,
            vars: vec![],
            body: None,
            type_idx: Some(0),
            local_names: vec![String::new(); 1],
        });

        // Function without type_idx (will be inferred)
        module.add_function(Function::new(
            "untyped".to_string(),
            Type::F32,
            Type::F64,
            vec![],
            None,
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
        // Both functions should have type_idx after parsing
        assert!(parsed.functions[0].type_idx.is_some());
        assert!(parsed.functions[1].type_idx.is_some());
    }

    #[test]
    fn test_code_section_many_locals() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test function with many local variables (100)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.const_(Literal::I32(42));
        let mut vars = Vec::new();
        for i in 0..100 {
            vars.push(if i % 2 == 0 { Type::I32 } else { Type::I64 });
        }

        let func = Function::new(
            "many_locals".to_string(),
            Type::NONE,
            Type::I32,
            vars,
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].vars.len(), 100);
    }

    #[test]
    fn test_code_section_function_with_params_and_locals() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test function with parameters and local variables
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.const_(Literal::I32(1));
        let func = Function::new(
            "params_and_locals".to_string(),
            Type::I32, // 1 param
            Type::I32,
            vec![Type::F32, Type::F64], // 2 locals
            Some(body),
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[0].vars.len(), 2);
    }

    #[test]
    fn test_roundtrip_all_sections_together() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Global, Import};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Test module with all major sections
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Types
        module.add_type(Type::I32, Type::I64);
        module.add_type(Type::F32, Type::F64);

        // Imports
        module.add_import(Import {
            module: "env".to_string(),
            name: "imported_func".to_string(),
            kind: crate::ir::module::ImportKind::Function(Type::I32, Type::I64),
        });

        // Functions
        let body = builder.const_(Literal::I32(42));
        module.add_function(Function::new(
            "local_func".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        // Globals
        let init = builder.const_(Literal::I32(100));
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init,
        });

        // Table
        module.table = Some(crate::ir::module::TableLimits {
            element_type: Type::FUNCREF,
            initial: 10,
            maximum: Some(100),
        });

        // Exports (export the local function, not the imported one)
        module.add_export(
            "exported_func".to_string(),
            crate::ir::module::ExportKind::Function,
            1, // Index 1 = first local function (after the import at index 0)
        );

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .expect("Failed to write complete module");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader
            .parse_module()
            .expect("Failed to parse complete module");

        // Types may include inferred types from functions
        assert!(parsed.types.len() >= 2, "Expected at least 2 types");
        assert_eq!(parsed.imports.len(), 1);
        assert_eq!(parsed.functions.len(), 1); // Only local functions
        assert_eq!(parsed.globals.len(), 1);
        assert!(parsed.table.is_some());
        assert_eq!(parsed.exports.len(), 1);
    }

    // ========== Import Section Tests ==========

    #[test]
    fn test_import_function() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_import(Import {
            module: "env".to_string(),
            name: "log".to_string(),
            kind: ImportKind::Function(Type::I32, Type::NONE),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.imports.len(), 1);
        assert_eq!(parsed.imports[0].module, "env");
        assert_eq!(parsed.imports[0].name, "log");
    }

    #[test]
    fn test_import_memory() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind, MemoryLimits};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_import(Import {
            module: "js".to_string(),
            name: "mem".to_string(),
            kind: ImportKind::Memory(MemoryLimits {
                initial: 1,
                maximum: Some(10),
            }),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.imports.len(), 1);
    }

    #[test]
    fn test_import_global() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_import(Import {
            module: "env".to_string(),
            name: "global_val".to_string(),
            kind: ImportKind::Global(Type::I32, false),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.imports.len(), 1);
    }

    #[test]
    fn test_multiple_imports_mixed() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind, MemoryLimits};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function import
        module.add_import(Import {
            module: "env".to_string(),
            name: "func1".to_string(),
            kind: ImportKind::Function(Type::I32, Type::I64),
        });

        // Another function import
        module.add_import(Import {
            module: "env".to_string(),
            name: "func2".to_string(),
            kind: ImportKind::Function(Type::NONE, Type::NONE),
        });

        // Global import
        module.add_import(Import {
            module: "js".to_string(),
            name: "g".to_string(),
            kind: ImportKind::Global(Type::F32, true),
        });

        // Memory import
        module.add_import(Import {
            module: "js".to_string(),
            name: "memory".to_string(),
            kind: ImportKind::Memory(MemoryLimits {
                initial: 2,
                maximum: None,
            }),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.imports.len(), 4);
    }

    #[test]
    fn test_import_many_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{Import, ImportKind};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        for i in 0..50 {
            module.add_import(Import {
                module: "env".to_string(),
                name: format!("func_{}", i),
                kind: ImportKind::Function(Type::I32, Type::I64),
            });
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.imports.len(), 50);
    }

    // ========== Export Section Tests ==========

    #[test]
    fn test_export_function() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::ExportKind;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a function
        module.add_function(Function::new(
            "my_func".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            None,
        ));

        // Export it
        module.add_export("exported".to_string(), ExportKind::Function, 0);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.exports.len(), 1);
        assert_eq!(parsed.exports[0].name, "exported");
    }

    #[test]
    fn test_export_multiple_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::ExportKind;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        for i in 0..10 {
            module.add_function(Function::new(
                format!("func_{}", i),
                Type::NONE,
                Type::I32,
                vec![],
                None,
            ));
            module.add_export(format!("export_{}", i), ExportKind::Function, i);
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.exports.len(), 10);
        assert_eq!(parsed.functions.len(), 10);
    }

    #[test]
    fn test_export_memory() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::ExportKind;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(1, Some(100));
        module.add_export("memory".to_string(), ExportKind::Memory, 0);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.exports.len(), 1);
        assert!(parsed.memory.is_some());
    }

    #[test]
    fn test_export_global() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{ExportKind, Global};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let init = builder.const_(Literal::I32(42));
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init,
        });

        module.add_export("my_global".to_string(), ExportKind::Global, 0);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.exports.len(), 1);
        assert_eq!(parsed.globals.len(), 1);
    }

    #[test]
    fn test_export_table() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{ExportKind, TableLimits};

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 10,
            maximum: Some(100),
        });

        module.add_export("my_table".to_string(), ExportKind::Table, 0);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.exports.len(), 1);
        assert!(parsed.table.is_some());
    }

    // ========== Memory Section Tests ==========

    #[test]
    fn test_memory_basic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(1, Some(10));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.memory.is_some());
        let mem = parsed.memory.unwrap();
        assert_eq!(mem.initial, 1);
        assert_eq!(mem.maximum, Some(10));
    }

    #[test]
    fn test_memory_no_maximum() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(5, None);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.memory.is_some());
        let mem = parsed.memory.unwrap();
        assert_eq!(mem.initial, 5);
        assert_eq!(mem.maximum, None);
    }

    #[test]
    fn test_memory_large_limits() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(100, Some(1000));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.memory.is_some());
        let mem = parsed.memory.unwrap();
        assert_eq!(mem.initial, 100);
        assert_eq!(mem.maximum, Some(1000));
    }

    // ========== Global Section Tests ==========

    #[test]
    fn test_global_immutable_i32() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let init = builder.const_(Literal::I32(123));
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init,
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.globals.len(), 1);
        assert_eq!(parsed.globals[0].type_, Type::I32);
        assert!(!parsed.globals[0].mutable);
    }

    #[test]
    fn test_global_mutable_i64() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let init = builder.const_(Literal::I64(999));
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I64,
            mutable: true,
            init,
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.globals.len(), 1);
        assert_eq!(parsed.globals[0].type_, Type::I64);
        assert!(parsed.globals[0].mutable);
    }

    #[test]
    fn test_global_all_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // i32 global
        let init1 = builder.const_(Literal::I32(1));
        module.add_global(Global {
            name: "g1".to_string(),
            type_: Type::I32,
            mutable: false,
            init: init1,
        });

        // i64 global
        let init2 = builder.const_(Literal::I64(2));
        module.add_global(Global {
            name: "g2".to_string(),
            type_: Type::I64,
            mutable: true,
            init: init2,
        });

        // f32 global
        let init3 = builder.const_(Literal::F32(std::f32::consts::PI));
        module.add_global(Global {
            name: "g3".to_string(),
            type_: Type::F32,
            mutable: false,
            init: init3,
        });

        // f64 global
        let init4 = builder.const_(Literal::F64(std::f64::consts::E));
        module.add_global(Global {
            name: "g4".to_string(),
            type_: Type::F64,
            mutable: true,
            init: init4,
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.globals.len(), 4);
    }

    #[test]
    fn test_global_many() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        for i in 0..100 {
            let init = builder.const_(Literal::I32(i));
            module.add_global(Global {
                name: format!("g{}", i),
                type_: Type::I32,
                mutable: i % 2 == 0,
                init,
            });
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.globals.len(), 100);
    }

    // ========== Element Section Tests ==========

    #[test]
    fn test_element_basic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{ElementSegment, TableLimits};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Need a table
        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 10,
            maximum: None,
        });

        // Need functions to reference
        module.add_function(Function::new(
            "f0".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            None,
        ));
        module.add_function(Function::new(
            "f1".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            None,
        ));

        // Element segment
        let offset = builder.const_(Literal::I32(0));
        module.elements.push(ElementSegment {
            table_index: 0,
            offset,
            func_indices: vec![0, 1],
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.elements.len(), 1);
        assert_eq!(parsed.elements[0].func_indices.len(), 2);
    }

    #[test]
    fn test_element_multiple_segments() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{ElementSegment, TableLimits};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 20,
            maximum: None,
        });

        for _ in 0..5 {
            module.add_function(Function::new(
                "f".to_string(),
                Type::NONE,
                Type::I32,
                vec![],
                None,
            ));
        }

        // Multiple segments at different offsets
        for i in 0..3 {
            let offset = builder.const_(Literal::I32(i * 5));
            module.elements.push(ElementSegment {
                table_index: 0,
                offset,
                func_indices: vec![0, 1],
            });
        }

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.elements.len(), 3);
    }

    #[test]
    fn test_element_many_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::{ElementSegment, TableLimits};

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 100,
            maximum: None,
        });

        for _ in 0..50 {
            module.add_function(Function::new(
                "f".to_string(),
                Type::NONE,
                Type::I32,
                vec![],
                None,
            ));
        }

        let offset = builder.const_(Literal::I32(0));
        let indices: Vec<u32> = (0..50).collect();
        module.elements.push(ElementSegment {
            table_index: 0,
            offset,
            func_indices: indices,
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.elements.len(), 1);
        assert_eq!(parsed.elements[0].func_indices.len(), 50);
    }

    // ========== Data Section Tests ==========

    #[test]
    fn test_data_basic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(1, None);

        let offset = builder.const_(Literal::I32(0));
        module.data.push(DataSegment {
            memory_index: 0,
            offset,
            data: vec![1, 2, 3, 4, 5],
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_data_multiple_segments() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(1, None);

        // Segment 1 at offset 0
        let offset1 = builder.const_(Literal::I32(0));
        module.data.push(DataSegment {
            memory_index: 0,
            offset: offset1,
            data: vec![1, 2, 3],
        });

        // Segment 2 at offset 100
        let offset2 = builder.const_(Literal::I32(100));
        module.data.push(DataSegment {
            memory_index: 0,
            offset: offset2,
            data: vec![4, 5, 6],
        });

        // Segment 3 at offset 200
        let offset3 = builder.const_(Literal::I32(200));
        module.data.push(DataSegment {
            memory_index: 0,
            offset: offset3,
            data: vec![7, 8, 9],
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.data.len(), 3);
    }

    #[test]
    fn test_data_large() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(10, None);

        let offset = builder.const_(Literal::I32(0));
        let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        module.data.push(DataSegment {
            memory_index: 0,
            offset,
            data: large_data.clone(),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].data.len(), 10000);
        assert_eq!(parsed.data[0].data, large_data);
    }

    #[test]
    fn test_data_empty() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::DataSegment;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.set_memory(1, None);

        let offset = builder.const_(Literal::I32(0));
        module.data.push(DataSegment {
            memory_index: 0,
            offset,
            data: vec![],
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].data.len(), 0);
    }

    // ========== Start Section Tests ==========

    #[test]
    fn test_start_function() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_function(Function::new(
            "start".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            None,
        ));
        module.start = Some(0);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.start, Some(0));
    }

    #[test]
    fn test_start_with_multiple_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        for i in 0..10 {
            module.add_function(Function::new(
                format!("func_{}", i),
                Type::NONE,
                Type::NONE,
                vec![],
                None,
            ));
        }

        module.start = Some(5); // Start at function 5

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.start, Some(5));
        assert_eq!(parsed.functions.len(), 10);
    }

    #[test]
    fn test_no_start_function() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.add_function(Function::new(
            "f".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            None,
        ));
        // No start set

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.start, None);
    }

    // ========== Table Section Tests ==========

    #[test]
    fn test_table_basic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::TableLimits;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 5,
            maximum: Some(50),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.table.is_some());
        let table = parsed.table.unwrap();
        assert_eq!(table.initial, 5);
        assert_eq!(table.maximum, Some(50));
    }

    #[test]
    fn test_table_no_maximum() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::TableLimits;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 10,
            maximum: None,
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.table.is_some());
        let table = parsed.table.unwrap();
        assert_eq!(table.initial, 10);
        assert_eq!(table.maximum, None);
    }

    #[test]
    fn test_table_large_limits() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::TableLimits;

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        module.table = Some(TableLimits {
            element_type: Type::FUNCREF,
            initial: 1000,
            maximum: Some(10000),
        });

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert!(parsed.table.is_some());
        let table = parsed.table.unwrap();
        assert_eq!(table.initial, 1000);
        assert_eq!(table.maximum, Some(10000));
    }

    // ========== Phase 1: Control Flow Instructions ==========

    #[test]
    fn test_unreachable_instruction() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.unreachable();
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert!(parsed.functions[0].body.is_some());
    }

    #[test]
    fn test_return_void() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let body = builder.return_(None);
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_return_with_value() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I32(42));
        let body = builder.return_(Some(value));
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_br_unconditional() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // block with unconditional break
        let br = builder.break_("label", None, None, Type::NONE);
        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(br);
        let body = builder.block(Some("label"), block_list, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_br_if_conditional() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // block with conditional break
        let condition = builder.const_(Literal::I32(1));
        let br_if = builder.break_("label", Some(condition), None, Type::NONE);
        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(br_if);
        let body = builder.block(Some("label"), block_list, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_br_with_value() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // block that returns a value via break
        let value = builder.const_(Literal::I32(42));
        let br = builder.break_("label", None, Some(value), Type::I32);
        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(br);
        let body = builder.block(Some("label"), block_list, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_nested_blocks_with_breaks() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // outer block
        let br_inner = builder.break_("inner", None, None, Type::NONE);
        let mut inner_list = BumpVec::new_in(&bump);
        inner_list.push(br_inner);
        let inner = builder.block(Some("inner"), inner_list, Type::NONE);

        let br_outer = builder.break_("outer", None, None, Type::NONE);
        let mut outer_list = BumpVec::new_in(&bump);
        outer_list.push(inner);
        outer_list.push(br_outer);
        let body = builder.block(Some("outer"), outer_list, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_loop_basic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let loop_body = builder.nop();
        let body = builder.loop_(Some("loop"), loop_body, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_loop_with_break_to_continue() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // break to loop label continues the loop
        let br = builder.break_("loop", None, None, Type::NONE);
        let body = builder.loop_(Some("loop"), br, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_loop_with_exit_block() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // break to outer block exits the loop
        let br = builder.break_("exit", None, None, Type::NONE);
        let loop_body = builder.loop_(Some("loop"), br, Type::NONE);
        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(loop_body);
        let body = builder.block(Some("exit"), block_list, Type::NONE);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_call_function() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function to be called
        module.add_function(Function::new(
            "callee".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(builder.const_(Literal::I32(42))),
        ));

        // Caller function
        let operands = BumpVec::new_in(&bump);
        let body = builder.call("callee", operands, Type::I32, false);
        module.add_function(Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
    }

    #[test]
    fn test_call_with_arguments() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Callee: takes two i32, returns i32
        let param1 = builder.local_get(0, Type::I32);
        let param2 = builder.local_get(1, Type::I32);
        let callee_body = builder.binary(BinaryOp::AddInt32, param1, param2, Type::I32);
        module.add_function(Function::new(
            "add".to_string(),
            Type::NONE, // params handled via add_type
            Type::I32,
            vec![Type::I32, Type::I32],
            Some(callee_body),
        ));

        // Caller: passes two constants
        let mut operands = BumpVec::new_in(&bump);
        operands.push(builder.const_(Literal::I32(10)));
        operands.push(builder.const_(Literal::I32(20)));
        let body = builder.call("add", operands, Type::I32, false);
        module.add_function(Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
    }

    #[test]
    fn test_tail_call() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Callee
        module.add_function(Function::new(
            "callee".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(builder.const_(Literal::I32(42))),
        ));

        // Caller with tail call (is_return=true)
        let operands = BumpVec::new_in(&bump);
        let body = builder.call("callee", operands, Type::I32, true);
        module.add_function(Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 2);
    }

    #[test]
    fn test_complex_control_flow() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // if (param0) { return 1; } else { return 0; }
        let condition = builder.local_get(0, Type::I32);
        let true_val = builder.const_(Literal::I32(1));
        let false_val = builder.const_(Literal::I32(0));
        let body = builder.if_(condition, true_val, Some(false_val), Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_deeply_nested_control_flow() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Nest 10 levels deep: block -> loop -> if -> block -> ...
        let mut expr = builder.const_(Literal::I32(42));
        for i in 0..10 {
            let name = bump.alloc_str(&format!("level{}", i));
            if i % 3 == 0 {
                let mut list = BumpVec::new_in(&bump);
                list.push(expr);
                expr = builder.block(Some(name), list, Type::I32);
            } else if i % 3 == 1 {
                expr = builder.loop_(Some(name), expr, Type::I32);
            } else {
                let condition = builder.const_(Literal::I32(1));
                expr = builder.if_(condition, expr, None, Type::I32);
            }
        }

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(expr),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_select_instruction() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let if_true = builder.const_(Literal::I32(10));
        let if_false = builder.const_(Literal::I32(20));
        let body = builder.select(condition, if_true, if_false, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_drop_instruction() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I32(42));
        let body = builder.drop(value);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_local_tee_instruction() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // local.tee sets a local and leaves value on stack
        let value = builder.const_(Literal::I32(42));
        let tee = builder.local_tee(0, value, Type::I32);
        let body = tee; // Return the tee'd value

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32], // One local
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ========== Phase 2: Parametric & Variable Instructions ==========

    #[test]
    fn test_multiple_local_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // local[0] = 10; local[1] = 20; return local[0] + local[1]
        let set0 = builder.local_set(0, builder.const_(Literal::I32(10)));
        let set1 = builder.local_set(1, builder.const_(Literal::I32(20)));
        let get0 = builder.local_get(0, Type::I32);
        let get1 = builder.local_get(1, Type::I32);
        let add = builder.binary(BinaryOp::AddInt32, get0, get1, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(set0);
        list.push(set1);
        list.push(add);
        let body = builder.block(None, list, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32, Type::I32],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_local_tee_chaining() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // local.tee 0 (local.tee 1 (const 42)) - sets both locals to 42
        let const_val = builder.const_(Literal::I32(42));
        let tee1 = builder.local_tee(1, const_val, Type::I32);
        let tee0 = builder.local_tee(0, tee1, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32, Type::I32],
            Some(tee0),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_local_swap_pattern() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Swap locals: temp = local[0]; local[0] = local[1]; local[1] = temp
        let get0 = builder.local_get(0, Type::I32);
        let tee_temp = builder.local_tee(2, get0, Type::I32);
        let drop_temp = builder.drop(tee_temp);
        let get1 = builder.local_get(1, Type::I32);
        let set0 = builder.local_set(0, get1);
        let get_temp = builder.local_get(2, Type::I32);
        let set1 = builder.local_set(1, get_temp);
        let result = builder.local_get(0, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(drop_temp);
        list.push(set0);
        list.push(set1);
        list.push(result);
        let body = builder.block(None, list, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32, Type::I32, Type::I32], // params + temp
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_select_with_f32() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let condition = builder.const_(Literal::I32(0));
        let if_true = builder.const_(Literal::F32(1.5));
        let if_false = builder.const_(Literal::F32(2.5));
        let body = builder.select(condition, if_true, if_false, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_select_with_i64() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let if_true = builder.const_(Literal::I64(100));
        let if_false = builder.const_(Literal::I64(200));
        let body = builder.select(condition, if_true, if_false, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_select_nested() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // select(c1, select(c2, a, b), c)
        let c2 = builder.const_(Literal::I32(1));
        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let inner = builder.select(c2, a, b, Type::I32);

        let c1 = builder.const_(Literal::I32(0));
        let c = builder.const_(Literal::I32(30));
        let body = builder.select(c1, inner, c, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_drop_multiple_values() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Drop multiple computed values
        let v1 = builder.const_(Literal::I32(10));
        let v2 = builder.const_(Literal::I32(20));
        let sum = builder.binary(BinaryOp::AddInt32, v1, v2, Type::I32);
        let drop1 = builder.drop(sum);

        let v3 = builder.const_(Literal::I64(100));
        let drop2 = builder.drop(v3);

        let result = builder.const_(Literal::I32(42));

        let mut list = BumpVec::new_in(&bump);
        list.push(drop1);
        list.push(drop2);
        list.push(result);
        let body = builder.block(None, list, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_global_operations_complex() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;
        use crate::ir::module::Global;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add two globals
        let init1 = builder.const_(Literal::I32(100));
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            init: init1,
        });

        let init2 = builder.const_(Literal::I32(200));
        module.add_global(Global {
            name: "g1".to_string(),
            type_: Type::I32,
            mutable: true,
            init: init2,
        });

        // Function: swap globals and return their sum
        let get0 = builder.global_get(0, Type::I32);
        let get1 = builder.global_get(1, Type::I32);
        let set0 = builder.global_set(0, get1);
        let set1 = builder.global_set(1, get0);
        let get0_new = builder.global_get(0, Type::I32);
        let get1_new = builder.global_get(1, Type::I32);
        let result = builder.binary(BinaryOp::AddInt32, get0_new, get1_new, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(set0);
        list.push(set1);
        list.push(result);
        let body = builder.block(None, list, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.globals.len(), 2);
    }

    #[test]
    fn test_many_locals_access() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Sum 10 locals
        let mut list = BumpVec::new_in(&bump);

        // Initialize locals
        for i in 0..10 {
            let set = builder.local_set(i, builder.const_(Literal::I32(i as i32)));
            list.push(set);
        }

        // Sum them all
        let mut sum = builder.local_get(0, Type::I32);
        for i in 1..10 {
            let get = builder.local_get(i, Type::I32);
            sum = builder.binary(BinaryOp::AddInt32, sum, get, Type::I32);
        }
        list.push(sum);

        let body = builder.block(None, list, Type::I32);

        let locals = vec![Type::I32; 10];
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            locals,
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ========== Phase 3: Integer Numeric Operations ==========

    // i32 arithmetic operations
    #[test]
    fn test_i32_arithmetic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Test arithmetic chain: (10 + 20) * 3 / 2 - 1
        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let add = builder.binary(BinaryOp::AddInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(3));
        let mul = builder.binary(BinaryOp::MulInt32, add, c, Type::I32);

        let d = builder.const_(Literal::I32(2));
        let div = builder.binary(BinaryOp::DivSInt32, mul, d, Type::I32);

        let e = builder.const_(Literal::I32(1));
        let body = builder.binary(BinaryOp::SubInt32, div, e, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_division_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I32(100));
        let b = builder.const_(Literal::I32(3));
        let div_s = builder.binary(BinaryOp::DivSInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(100));
        let d = builder.const_(Literal::I32(3));
        let div_u = builder.binary(BinaryOp::DivUInt32, c, d, Type::I32);

        let body = builder.binary(BinaryOp::AddInt32, div_s, div_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_remainder() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I32(17));
        let b = builder.const_(Literal::I32(5));
        let rem_s = builder.binary(BinaryOp::RemSInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(17));
        let d = builder.const_(Literal::I32(5));
        let rem_u = builder.binary(BinaryOp::RemUInt32, c, d, Type::I32);

        let body = builder.binary(BinaryOp::MulInt32, rem_s, rem_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_bitwise() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I32(0xFF00));
        let b = builder.const_(Literal::I32(0x0FF0));
        let and = builder.binary(BinaryOp::AndInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(0xFF00));
        let d = builder.const_(Literal::I32(0x0FF0));
        let or = builder.binary(BinaryOp::OrInt32, c, d, Type::I32);

        let e = builder.const_(Literal::I32(0xFF00));
        let f = builder.const_(Literal::I32(0x0FF0));
        let xor = builder.binary(BinaryOp::XorInt32, e, f, Type::I32);

        let r1 = builder.binary(BinaryOp::OrInt32, and, or, Type::I32);
        let body = builder.binary(BinaryOp::XorInt32, r1, xor, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_shifts() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I32(0x12345678));
        let amt = builder.const_(Literal::I32(4));
        let shl = builder.binary(BinaryOp::ShlInt32, value, amt, Type::I32);

        let value2 = builder.const_(Literal::I32(0x12345678));
        let amt2 = builder.const_(Literal::I32(4));
        let shr_s = builder.binary(BinaryOp::ShrSInt32, value2, amt2, Type::I32);

        let value3 = builder.const_(Literal::I32(0x12345678));
        let amt3 = builder.const_(Literal::I32(4));
        let shr_u = builder.binary(BinaryOp::ShrUInt32, value3, amt3, Type::I32);

        let r1 = builder.binary(BinaryOp::XorInt32, shl, shr_s, Type::I32);
        let body = builder.binary(BinaryOp::XorInt32, r1, shr_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_rotates() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I32(0xABCD1234u32 as i32));
        let amt = builder.const_(Literal::I32(8));
        let rotl = builder.binary(BinaryOp::RotLInt32, value, amt, Type::I32);

        let value2 = builder.const_(Literal::I32(0xABCD1234u32 as i32));
        let amt2 = builder.const_(Literal::I32(8));
        let rotr = builder.binary(BinaryOp::RotRInt32, value2, amt2, Type::I32);

        let body = builder.binary(BinaryOp::XorInt32, rotl, rotr, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_comparisons() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let eq = builder.binary(BinaryOp::EqInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(10));
        let d = builder.const_(Literal::I32(20));
        let ne = builder.binary(BinaryOp::NeInt32, c, d, Type::I32);

        let e = builder.const_(Literal::I32(10));
        let f = builder.const_(Literal::I32(20));
        let lt_s = builder.binary(BinaryOp::LtSInt32, e, f, Type::I32);

        let g = builder.const_(Literal::I32(10));
        let h = builder.const_(Literal::I32(20));
        let le_u = builder.binary(BinaryOp::LeUInt32, g, h, Type::I32);

        let r1 = builder.binary(BinaryOp::AndInt32, eq, ne, Type::I32);
        let r2 = builder.binary(BinaryOp::AndInt32, lt_s, le_u, Type::I32);
        let body = builder.binary(BinaryOp::OrInt32, r1, r2, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_comparison_variants() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let gt_s = builder.binary(BinaryOp::GtSInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(10));
        let d = builder.const_(Literal::I32(20));
        let gt_u = builder.binary(BinaryOp::GtUInt32, c, d, Type::I32);

        let e = builder.const_(Literal::I32(10));
        let f = builder.const_(Literal::I32(20));
        let ge_s = builder.binary(BinaryOp::GeSInt32, e, f, Type::I32);

        let g = builder.const_(Literal::I32(10));
        let h = builder.const_(Literal::I32(20));
        let ge_u = builder.binary(BinaryOp::GeUInt32, g, h, Type::I32);

        let r1 = builder.binary(BinaryOp::AddInt32, gt_s, gt_u, Type::I32);
        let r2 = builder.binary(BinaryOp::AddInt32, ge_s, ge_u, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, r1, r2, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_unary_all() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value1 = builder.const_(Literal::I32(0x12340000));
        let value2 = builder.const_(Literal::I32(0x12340000));
        let value3 = builder.const_(Literal::I32(0x12340000));
        let value4 = builder.const_(Literal::I32(0x12340000));

        let clz = builder.unary(UnaryOp::ClzInt32, value1, Type::I32);
        let ctz = builder.unary(UnaryOp::CtzInt32, value2, Type::I32);
        let popcnt = builder.unary(UnaryOp::PopcntInt32, value3, Type::I32);
        let eqz = builder.unary(UnaryOp::EqZInt32, value4, Type::I32);

        // Combine: clz + ctz + popcnt + eqz
        let r1 = builder.binary(BinaryOp::AddInt32, clz, ctz, Type::I32);
        let r2 = builder.binary(BinaryOp::AddInt32, r1, popcnt, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, r2, eqz, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // i64 operations
    #[test]
    fn test_i64_arithmetic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I64(1000));
        let b = builder.const_(Literal::I64(300));
        let add = builder.binary(BinaryOp::AddInt64, a, b, Type::I64);

        let c = builder.const_(Literal::I64(2000));
        let d = builder.const_(Literal::I64(500));
        let sub = builder.binary(BinaryOp::SubInt64, c, d, Type::I64);

        let mul = builder.binary(BinaryOp::MulInt64, add, sub, Type::I64);

        let e = builder.const_(Literal::I64(10));
        let body = builder.binary(BinaryOp::DivSInt64, mul, e, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_bitwise() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I64(0xFFFF0000FFFF0000u64 as i64));
        let b = builder.const_(Literal::I64(0x0000FFFF0000FFFFu64 as i64));
        let and = builder.binary(BinaryOp::AndInt64, a, b, Type::I64);

        let c = builder.const_(Literal::I64(0xFFFF0000FFFF0000u64 as i64));
        let d = builder.const_(Literal::I64(0x0000FFFF0000FFFFu64 as i64));
        let or = builder.binary(BinaryOp::OrInt64, c, d, Type::I64);

        let e = builder.const_(Literal::I64(0xFFFF0000FFFF0000u64 as i64));
        let f = builder.const_(Literal::I64(0x0000FFFF0000FFFFu64 as i64));
        let xor = builder.binary(BinaryOp::XorInt64, e, f, Type::I64);

        let r1 = builder.binary(BinaryOp::AndInt64, and, or, Type::I64);
        let body = builder.binary(BinaryOp::XorInt64, r1, xor, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_shifts() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I64(0x123456789ABCDEF0u64 as i64));
        let amt = builder.const_(Literal::I64(8));
        let shl = builder.binary(BinaryOp::ShlInt64, value, amt, Type::I64);

        let value2 = builder.const_(Literal::I64(0x123456789ABCDEF0u64 as i64));
        let amt2 = builder.const_(Literal::I64(8));
        let shr_s = builder.binary(BinaryOp::ShrSInt64, value2, amt2, Type::I64);

        let value3 = builder.const_(Literal::I64(0x123456789ABCDEF0u64 as i64));
        let amt3 = builder.const_(Literal::I64(8));
        let shr_u = builder.binary(BinaryOp::ShrUInt64, value3, amt3, Type::I64);

        let r1 = builder.binary(BinaryOp::XorInt64, shl, shr_s, Type::I64);
        let body = builder.binary(BinaryOp::XorInt64, r1, shr_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_rotates() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I64(0xFEDCBA9876543210u64 as i64));
        let amt = builder.const_(Literal::I64(16));
        let rotl = builder.binary(BinaryOp::RotLInt64, value, amt, Type::I64);

        let value2 = builder.const_(Literal::I64(0xFEDCBA9876543210u64 as i64));
        let amt2 = builder.const_(Literal::I64(16));
        let rotr = builder.binary(BinaryOp::RotRInt64, value2, amt2, Type::I64);

        let body = builder.binary(BinaryOp::XorInt64, rotl, rotr, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_comparisons() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::I64(1000));
        let b = builder.const_(Literal::I64(2000));
        let eq = builder.binary(BinaryOp::EqInt64, a, b, Type::I32);

        let c = builder.const_(Literal::I64(1000));
        let d = builder.const_(Literal::I64(2000));
        let ne = builder.binary(BinaryOp::NeInt64, c, d, Type::I32);

        let e = builder.const_(Literal::I64(1000));
        let f = builder.const_(Literal::I64(2000));
        let lt_s = builder.binary(BinaryOp::LtSInt64, e, f, Type::I32);

        let g = builder.const_(Literal::I64(1000));
        let h = builder.const_(Literal::I64(2000));
        let ge_u = builder.binary(BinaryOp::GeUInt64, g, h, Type::I32);

        let r1 = builder.binary(BinaryOp::AddInt32, eq, ne, Type::I32);
        let r2 = builder.binary(BinaryOp::AddInt32, lt_s, ge_u, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, r1, r2, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_unary_all() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value1 = builder.const_(Literal::I64(0x1234000000000000u64 as i64));
        let value2 = builder.const_(Literal::I64(0x1234000000000000u64 as i64));
        let value3 = builder.const_(Literal::I64(0x1234000000000000u64 as i64));
        let value4 = builder.const_(Literal::I64(0x1234000000000000u64 as i64));

        let clz = builder.unary(UnaryOp::ClzInt64, value1, Type::I64);
        let ctz = builder.unary(UnaryOp::CtzInt64, value2, Type::I64);
        let popcnt = builder.unary(UnaryOp::PopcntInt64, value3, Type::I64);
        let eqz = builder.unary(UnaryOp::EqZInt64, value4, Type::I64);

        let r1 = builder.binary(BinaryOp::AddInt64, clz, ctz, Type::I64);
        let r2 = builder.binary(BinaryOp::AddInt64, r1, popcnt, Type::I64);
        let body = builder.binary(BinaryOp::AddInt64, r2, eqz, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_mixed_i32_i64_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Use i32 for comparison, i64 for computation
        let i32_val = builder.const_(Literal::I32(100));
        let i64_a = builder.const_(Literal::I64(1000));
        let i64_b = builder.const_(Literal::I64(2000));

        let i64_sum = builder.binary(BinaryOp::AddInt64, i64_a, i64_b, Type::I64);
        let comparison = builder.unary(UnaryOp::EqZInt32, i32_val, Type::I32);

        // Select between sum and constant based on comparison
        let fallback = builder.const_(Literal::I64(9999));
        let body = builder.select(comparison, i64_sum, fallback, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_extreme_integer_values() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Test with extreme values
        let i32_max = builder.const_(Literal::I32(i32::MAX));
        let i32_min = builder.const_(Literal::I32(i32::MIN));
        let i32_zero = builder.const_(Literal::I32(0));
        let i32_neg_one = builder.const_(Literal::I32(-1));

        let i64_max = builder.const_(Literal::I64(i64::MAX));
        let i64_min = builder.const_(Literal::I64(i64::MIN));

        // Operations with extreme values
        let r1 = builder.binary(BinaryOp::AddInt32, i32_max, i32_min, Type::I32);
        let r2 = builder.binary(BinaryOp::XorInt32, i32_zero, i32_neg_one, Type::I32);
        let r3 = builder.binary(BinaryOp::AndInt32, r1, r2, Type::I32);

        let r4 = builder.binary(BinaryOp::AddInt64, i64_max, i64_min, Type::I64);
        let comparison = builder.unary(UnaryOp::EqZInt32, r3, Type::I32);

        let body = builder.select(comparison, r4, builder.const_(Literal::I64(0)), Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ============================================================================
    // Phase 4: Float Numeric Operations
    // ============================================================================

    #[test]
    fn test_f32_arithmetic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F32(10.5));
        let b = builder.const_(Literal::F32(20.3));
        let add = builder.binary(BinaryOp::AddFloat32, a, b, Type::F32);

        let c = builder.const_(Literal::F32(3.0));
        let mul = builder.binary(BinaryOp::MulFloat32, add, c, Type::F32);

        let d = builder.const_(Literal::F32(2.0));
        let div = builder.binary(BinaryOp::DivFloat32, mul, d, Type::F32);

        let e = builder.const_(Literal::F32(1.5));
        let body = builder.binary(BinaryOp::SubFloat32, div, e, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_comparisons() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F32(10.5));
        let b = builder.const_(Literal::F32(20.3));
        let eq = builder.binary(BinaryOp::EqFloat32, a, b, Type::I32);

        let c = builder.const_(Literal::F32(5.0));
        let d = builder.const_(Literal::F32(10.0));
        let lt = builder.binary(BinaryOp::LtFloat32, c, d, Type::I32);

        let e = builder.const_(Literal::F32(15.0));
        let f = builder.const_(Literal::F32(10.0));
        let gt = builder.binary(BinaryOp::GtFloat32, e, f, Type::I32);

        let r1 = builder.binary(BinaryOp::AddInt32, eq, lt, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, r1, gt, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_math_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let val1 = builder.const_(Literal::F32(-42.7));
        let abs_val = builder.unary(UnaryOp::AbsFloat32, val1, Type::F32);

        let val2 = builder.const_(Literal::F32(std::f32::consts::PI));
        let neg_val = builder.unary(UnaryOp::NegFloat32, val2, Type::F32);

        let val3 = builder.const_(Literal::F32(4.7));
        let ceil_val = builder.unary(UnaryOp::CeilFloat32, val3, Type::F32);

        let val4 = builder.const_(Literal::F32(4.7));
        let floor_val = builder.unary(UnaryOp::FloorFloat32, val4, Type::F32);

        let r1 = builder.binary(BinaryOp::AddFloat32, abs_val, neg_val, Type::F32);
        let r2 = builder.binary(BinaryOp::AddFloat32, ceil_val, floor_val, Type::F32);
        let body = builder.binary(BinaryOp::AddFloat32, r1, r2, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_advanced_math() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let val1 = builder.const_(Literal::F32(4.8));
        let trunc_val = builder.unary(UnaryOp::TruncFloat32, val1, Type::F32);

        let val2 = builder.const_(Literal::F32(4.5));
        let nearest = builder.unary(UnaryOp::NearestFloat32, val2, Type::F32);

        let val3 = builder.const_(Literal::F32(16.0));
        let sqrt_val = builder.unary(UnaryOp::SqrtFloat32, val3, Type::F32);

        let r1 = builder.binary(BinaryOp::AddFloat32, trunc_val, nearest, Type::F32);
        let body = builder.binary(BinaryOp::AddFloat32, r1, sqrt_val, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_min_max() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F32(10.5));
        let b = builder.const_(Literal::F32(20.3));
        let min_val = builder.binary(BinaryOp::MinFloat32, a, b, Type::F32);

        let c = builder.const_(Literal::F32(5.0));
        let d = builder.const_(Literal::F32(15.0));
        let max_val = builder.binary(BinaryOp::MaxFloat32, c, d, Type::F32);

        let body = builder.binary(BinaryOp::AddFloat32, min_val, max_val, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_copysign() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F32(42.0));
        let b = builder.const_(Literal::F32(-1.0));
        let body = builder.binary(BinaryOp::CopySignFloat32, a, b, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_arithmetic() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F64(100.5));
        let b = builder.const_(Literal::F64(200.3));
        let add = builder.binary(BinaryOp::AddFloat64, a, b, Type::F64);

        let c = builder.const_(Literal::F64(3.0));
        let mul = builder.binary(BinaryOp::MulFloat64, add, c, Type::F64);

        let d = builder.const_(Literal::F64(2.0));
        let div = builder.binary(BinaryOp::DivFloat64, mul, d, Type::F64);

        let e = builder.const_(Literal::F64(1.5));
        let body = builder.binary(BinaryOp::SubFloat64, div, e, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_comparisons() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F64(10.5));
        let b = builder.const_(Literal::F64(20.3));
        let ne = builder.binary(BinaryOp::NeFloat64, a, b, Type::I32);

        let c = builder.const_(Literal::F64(5.0));
        let d = builder.const_(Literal::F64(10.0));
        let le = builder.binary(BinaryOp::LeFloat64, c, d, Type::I32);

        let e = builder.const_(Literal::F64(15.0));
        let f = builder.const_(Literal::F64(10.0));
        let ge = builder.binary(BinaryOp::GeFloat64, e, f, Type::I32);

        let r1 = builder.binary(BinaryOp::AddInt32, ne, le, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, r1, ge, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_math_functions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let val1 = builder.const_(Literal::F64(-142.7));
        let abs_val = builder.unary(UnaryOp::AbsFloat64, val1, Type::F64);

        let val2 = builder.const_(Literal::F64(std::f64::consts::PI));
        let neg_val = builder.unary(UnaryOp::NegFloat64, val2, Type::F64);

        let val3 = builder.const_(Literal::F64(64.0));
        let sqrt_val = builder.unary(UnaryOp::SqrtFloat64, val3, Type::F64);

        let r1 = builder.binary(BinaryOp::AddFloat64, abs_val, neg_val, Type::F64);
        let body = builder.binary(BinaryOp::AddFloat64, r1, sqrt_val, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_rounding() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let val1 = builder.const_(Literal::F64(4.8));
        let ceil_val = builder.unary(UnaryOp::CeilFloat64, val1, Type::F64);

        let val2 = builder.const_(Literal::F64(4.8));
        let floor_val = builder.unary(UnaryOp::FloorFloat64, val2, Type::F64);

        let val3 = builder.const_(Literal::F64(4.8));
        let trunc_val = builder.unary(UnaryOp::TruncFloat64, val3, Type::F64);

        let val4 = builder.const_(Literal::F64(4.5));
        let nearest = builder.unary(UnaryOp::NearestFloat64, val4, Type::F64);

        let r1 = builder.binary(BinaryOp::AddFloat64, ceil_val, floor_val, Type::F64);
        let r2 = builder.binary(BinaryOp::AddFloat64, trunc_val, nearest, Type::F64);
        let body = builder.binary(BinaryOp::AddFloat64, r1, r2, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_min_max() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let a = builder.const_(Literal::F64(100.5));
        let b = builder.const_(Literal::F64(200.3));
        let min_val = builder.binary(BinaryOp::MinFloat64, a, b, Type::F64);

        let c = builder.const_(Literal::F64(50.0));
        let d = builder.const_(Literal::F64(150.0));
        let max_val = builder.binary(BinaryOp::MaxFloat64, c, d, Type::F64);

        let body = builder.binary(BinaryOp::AddFloat64, min_val, max_val, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_mixed_f32_f64_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let f32_a = builder.const_(Literal::F32(10.5));
        let f32_b = builder.const_(Literal::F32(5.0));
        let f32_result = builder.binary(BinaryOp::MulFloat32, f32_a, f32_b, Type::F32);

        let f64_a = builder.const_(Literal::F64(100.0));
        let f64_b = builder.const_(Literal::F64(50.0));
        let f64_result = builder.binary(BinaryOp::DivFloat64, f64_a, f64_b, Type::F64);

        // Compare f32 result with constant
        let f32_compare = builder.const_(Literal::F32(50.0));
        let cmp = builder.binary(BinaryOp::GtFloat32, f32_result, f32_compare, Type::I32);

        // Select between f64_result and a constant based on comparison
        let alt = builder.const_(Literal::F64(999.0));
        let body = builder.select(cmp, f64_result, alt, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ============================================================================
    // Phase 5: Conversion & Reinterpret Operations
    // ============================================================================

    #[test]
    fn test_i32_to_f32_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let signed = builder.const_(Literal::I32(-42));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt32ToFloat32, signed, Type::F32);

        let unsigned = builder.const_(Literal::I32(100));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt32ToFloat32, unsigned, Type::F32);

        let body = builder.binary(BinaryOp::AddFloat32, s_to_f, u_to_f, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_to_f32_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let signed = builder.const_(Literal::I64(-1000));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt64ToFloat32, signed, Type::F32);

        let unsigned = builder.const_(Literal::I64(2000));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt64ToFloat32, unsigned, Type::F32);

        let body = builder.binary(BinaryOp::AddFloat32, s_to_f, u_to_f, Type::F32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_to_f64_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let signed = builder.const_(Literal::I32(-42));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt32ToFloat64, signed, Type::F64);

        let unsigned = builder.const_(Literal::I32(100));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt32ToFloat64, unsigned, Type::F64);

        let body = builder.binary(BinaryOp::AddFloat64, s_to_f, u_to_f, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_to_f64_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let signed = builder.const_(Literal::I64(-1000));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt64ToFloat64, signed, Type::F64);

        let unsigned = builder.const_(Literal::I64(2000));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt64ToFloat64, unsigned, Type::F64);

        let body = builder.binary(BinaryOp::AddFloat64, s_to_f, u_to_f, Type::F64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_to_i32_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let float_val = builder.const_(Literal::F32(42.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat32ToInt32, float_val, Type::I32);

        let float_val2 = builder.const_(Literal::F32(100.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat32ToInt32, float_val2, Type::I32);

        let body = builder.binary(BinaryOp::AddInt32, trunc_s, trunc_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_to_i32_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let float_val = builder.const_(Literal::F64(42.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat64ToInt32, float_val, Type::I32);

        let float_val2 = builder.const_(Literal::F64(100.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat64ToInt32, float_val2, Type::I32);

        let body = builder.binary(BinaryOp::AddInt32, trunc_s, trunc_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_to_i64_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let float_val = builder.const_(Literal::F32(142.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat32ToInt64, float_val, Type::I64);

        let float_val2 = builder.const_(Literal::F32(200.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat32ToInt64, float_val2, Type::I64);

        let body = builder.binary(BinaryOp::AddInt64, trunc_s, trunc_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_to_i64_conversions() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let float_val = builder.const_(Literal::F64(142.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat64ToInt64, float_val, Type::I64);

        let float_val2 = builder.const_(Literal::F64(200.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat64ToInt64, float_val2, Type::I64);

        let body = builder.binary(BinaryOp::AddInt64, trunc_s, trunc_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ============================================================================
    // Phase 6: Memory Operations
    // ============================================================================

    #[test]
    fn test_i32_load_store() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Load from address 0, add 10, store back
        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(4, false, 0, 2, addr, Type::I32);

        let ten = builder.const_(Literal::I32(10));
        let added = builder.binary(BinaryOp::AddInt32, loaded, ten, Type::I32);

        let store_addr = builder.const_(Literal::I32(0));
        let body = builder.store(4, 0, 2, store_addr, added);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_load_store() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr = builder.const_(Literal::I32(8));
        let loaded = builder.load(8, false, 0, 3, addr, Type::I64);

        let multiplier = builder.const_(Literal::I64(2));
        let result = builder.binary(BinaryOp::MulInt64, loaded, multiplier, Type::I64);

        let store_addr = builder.const_(Literal::I32(16));
        let body = builder.store(8, 0, 3, store_addr, result);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_load8_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(1, true, 0, 0, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(1));
        let load_u = builder.load(1, false, 0, 0, addr2, Type::I32);

        let body = builder.binary(BinaryOp::AddInt32, load_s, load_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_load16_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(2, true, 0, 1, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(2));
        let load_u = builder.load(2, false, 0, 1, addr2, Type::I32);

        let body = builder.binary(BinaryOp::SubInt32, load_s, load_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_load8_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(1, true, 0, 0, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(1));
        let load_u = builder.load(1, false, 0, 0, addr2, Type::I64);

        let body = builder.binary(BinaryOp::AddInt64, load_s, load_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_load16_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(2, true, 0, 1, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(2));
        let load_u = builder.load(2, false, 0, 1, addr2, Type::I64);

        let body = builder.binary(BinaryOp::SubInt64, load_s, load_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_load32_signed_unsigned() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(4, true, 0, 2, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(4));
        let load_u = builder.load(4, false, 0, 2, addr2, Type::I64);

        let body = builder.binary(BinaryOp::MulInt64, load_s, load_u, Type::I64);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f32_load_store() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(4, false, 0, 2, addr, Type::F32);

        let multiplier = builder.const_(Literal::F32(2.0));
        let result = builder.binary(BinaryOp::MulFloat32, loaded, multiplier, Type::F32);

        let store_addr = builder.const_(Literal::I32(4));
        let body = builder.store(4, 0, 2, store_addr, result);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_f64_load_store() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(8, false, 0, 3, addr, Type::F64);

        let divisor = builder.const_(Literal::F64(2.0));
        let result = builder.binary(BinaryOp::DivFloat64, loaded, divisor, Type::F64);

        let store_addr = builder.const_(Literal::I32(8));
        let body = builder.store(8, 0, 3, store_addr, result);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i32_store8_store16() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value8 = builder.const_(Literal::I32(255));
        let addr8 = builder.const_(Literal::I32(0));
        let store8 = builder.store(1, 0, 0, addr8, value8);

        let value16 = builder.const_(Literal::I32(65535));
        let addr16 = builder.const_(Literal::I32(4));
        let store16 = builder.store(2, 0, 1, addr16, value16);

        let body = builder.block(
            None,
            BumpVec::from_iter_in([store8, store16], builder.bump),
            Type::NONE,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_i64_store8_store16_store32() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value8 = builder.const_(Literal::I64(255));
        let addr8 = builder.const_(Literal::I32(0));
        let store8 = builder.store(1, 0, 0, addr8, value8);

        let value16 = builder.const_(Literal::I64(65535));
        let addr16 = builder.const_(Literal::I32(8));
        let store16 = builder.store(2, 0, 1, addr16, value16);

        let value32 = builder.const_(Literal::I64(0xFFFFFFFF));
        let addr32 = builder.const_(Literal::I32(16));
        let store32 = builder.store(4, 0, 2, addr32, value32);

        let body = builder.block(
            None,
            BumpVec::from_iter_in([store8, store16, store32], builder.bump),
            Type::NONE,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_load_with_offset() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let base = builder.const_(Literal::I32(100));
        let loaded1 = builder.load(4, false, 0, 2, base, Type::I32);

        let base2 = builder.const_(Literal::I32(100));
        let loaded2 = builder.load(4, false, 16, 2, base2, Type::I32);

        let body = builder.binary(BinaryOp::AddInt32, loaded1, loaded2, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_store_with_offset() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let value = builder.const_(Literal::I32(42));
        let base = builder.const_(Literal::I32(1000));
        let body = builder.store(4, 64, 2, base, value);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_memory_size() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let size = builder.memory_size();
        let multiplier = builder.const_(Literal::I32(65536));
        let body = builder.binary(BinaryOp::MulInt32, size, multiplier, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_memory_grow() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let pages = builder.const_(Literal::I32(1));
        let old_size = builder.memory_grow(pages);

        let new_size = builder.memory_size();
        let body = builder.binary(BinaryOp::AddInt32, old_size, new_size, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_complex_memory_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Load i32, i64, f32, f64 from different addresses
        let addr1 = builder.const_(Literal::I32(0));
        let i32_val = builder.load(4, false, 0, 2, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(8));
        let _i64_val = builder.load(8, false, 0, 3, addr2, Type::I64);

        let addr3 = builder.const_(Literal::I32(16));
        let f32_val = builder.load(4, false, 0, 2, addr3, Type::F32);

        // Do some operations
        let i32_converted = builder.unary(UnaryOp::ConvertSInt32ToFloat32, i32_val, Type::F32);
        let sum = builder.binary(BinaryOp::AddFloat32, i32_converted, f32_val, Type::F32);

        // Store result
        let store_addr = builder.const_(Literal::I32(32));
        let body = builder.store(4, 0, 2, store_addr, sum);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    // ============================================================================
    // Phase 7: Validation & Edge Cases
    // ============================================================================

    #[test]
    fn test_deeply_nested_blocks() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Create 20 levels of nested blocks
        let value = builder.const_(Literal::I32(42));
        let mut current = value;

        for i in 0..20 {
            let label = format!("block_{}", i);
            let label_str =
                bumpalo::collections::String::from_str_in(&label, &bump).into_bump_str();
            current = builder.block(
                Some(label_str),
                BumpVec::from_iter_in([current], &bump),
                Type::I32,
            );
        }

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(current),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_deeply_nested_ifs() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Create 15 levels of nested if statements
        let mut current = builder.const_(Literal::I32(1));

        for _ in 0..15 {
            let condition = builder.const_(Literal::I32(1));
            let false_branch = builder.const_(Literal::I32(0));
            current = builder.if_(condition, current, Some(false_branch), Type::I32);
        }

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(current),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_all_operations_combined() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Integer operations
        let i32_a = builder.const_(Literal::I32(10));
        let i32_b = builder.const_(Literal::I32(20));
        let i32_sum = builder.binary(BinaryOp::AddInt32, i32_a, i32_b, Type::I32);
        let i32_mul = builder.binary(
            BinaryOp::MulInt32,
            i32_sum,
            builder.const_(Literal::I32(2)),
            Type::I32,
        );

        // Float operations
        let f32_a = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_b = builder.const_(Literal::F32(2.0));
        let f32_div = builder.binary(BinaryOp::DivFloat32, f32_a, f32_b, Type::F32);
        let f32_sqrt = builder.unary(UnaryOp::SqrtFloat32, f32_div, Type::F32);

        // Conversions
        let i32_to_f32 = builder.unary(UnaryOp::ConvertSInt32ToFloat32, i32_mul, Type::F32);
        let f32_combined = builder.binary(BinaryOp::AddFloat32, i32_to_f32, f32_sqrt, Type::F32);
        let f32_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, f32_combined, Type::I32);

        // Memory operations
        let store_addr = builder.const_(Literal::I32(0));
        let store_op = builder.store(4, 0, 2, store_addr, f32_to_i32);

        let load_addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(4, false, 0, 2, load_addr, Type::I32);

        // Control flow
        let condition = builder.binary(
            BinaryOp::GtSInt32,
            loaded,
            builder.const_(Literal::I32(50)),
            Type::I32,
        );
        let if_true = builder.const_(Literal::I32(100));
        let if_false = builder.const_(Literal::I32(200));
        let selected = builder.if_(condition, if_true, Some(if_false), Type::I32);

        // Combine everything in a block
        let body = builder.block(
            None,
            BumpVec::from_iter_in([store_op, selected], &bump),
            Type::I32,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_complex_expression_chain() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Build a complex expression chain with 50+ operations
        let mut result = builder.const_(Literal::I32(1));

        for i in 1..=25 {
            let next_val = builder.const_(Literal::I32(i));
            result = builder.binary(BinaryOp::AddInt32, result, next_val, Type::I32);

            let multiplier = builder.const_(Literal::I32(2));
            result = builder.binary(BinaryOp::MulInt32, result, multiplier, Type::I32);
        }

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(result),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_mixed_type_operations_i32_i64() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Mix i32 and i64 operations with proper type handling
        let i32_val = builder.const_(Literal::I32(42));
        let i32_doubled = builder.binary(
            BinaryOp::MulInt32,
            i32_val,
            builder.const_(Literal::I32(2)),
            Type::I32,
        );

        let i64_val1 = builder.const_(Literal::I64(1000));
        let i64_val2 = builder.const_(Literal::I64(1000));
        let i64_squared = builder.binary(BinaryOp::MulInt64, i64_val1, i64_val2, Type::I64);

        // Use select to choose between the results (need same type)
        let i32_compare = builder.binary(
            BinaryOp::GtSInt32,
            i32_doubled,
            builder.const_(Literal::I32(50)),
            Type::I32,
        );
        let body = builder.select(
            i32_compare,
            i64_squared,
            builder.const_(Literal::I64(999)),
            Type::I64,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_mixed_type_operations_floats() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Mix f32 and f64 operations
        let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_result = builder.unary(UnaryOp::SqrtFloat32, f32_val, Type::F32);

        let f64_val1 = builder.const_(Literal::F64(std::f64::consts::E));
        let f64_val2 = builder.const_(Literal::F64(std::f64::consts::E));
        let f64_result = builder.binary(BinaryOp::MulFloat64, f64_val1, f64_val2, Type::F64);

        // Compare f32 value
        let f32_cmp = builder.binary(
            BinaryOp::GtFloat32,
            f32_result,
            builder.const_(Literal::F32(1.0)),
            Type::I32,
        );
        let body = builder.select(
            f32_cmp,
            f64_result,
            builder.const_(Literal::F64(1.0)),
            Type::F64,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::F64,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_all_unary_operations_combined() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // i32 unary ops
        let i32_val = builder.const_(Literal::I32(0xFF00FF00u32 as i32));
        let clz = builder.unary(UnaryOp::ClzInt32, i32_val, Type::I32);
        let ctz = builder.unary(UnaryOp::CtzInt32, clz, Type::I32);
        let popcnt = builder.unary(UnaryOp::PopcntInt32, ctz, Type::I32);

        // f32 unary ops
        let f32_val = builder.const_(Literal::F32(-42.7));
        let abs_val = builder.unary(UnaryOp::AbsFloat32, f32_val, Type::F32);
        let ceil_val = builder.unary(UnaryOp::CeilFloat32, abs_val, Type::F32);
        let floor_val = builder.unary(UnaryOp::FloorFloat32, ceil_val, Type::F32);
        let sqrt_val = builder.unary(UnaryOp::SqrtFloat32, floor_val, Type::F32);

        // Convert and combine
        let f32_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, sqrt_val, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, popcnt, f32_to_i32, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_all_comparison_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // i32 comparisons
        let a1 = builder.const_(Literal::I32(10));
        let b1 = builder.const_(Literal::I32(20));
        let eq = builder.binary(BinaryOp::EqInt32, a1, b1, Type::I32);

        let a2 = builder.const_(Literal::I32(10));
        let b2 = builder.const_(Literal::I32(20));
        let ne = builder.binary(BinaryOp::NeInt32, a2, b2, Type::I32);

        let a3 = builder.const_(Literal::I32(10));
        let b3 = builder.const_(Literal::I32(20));
        let lt = builder.binary(BinaryOp::LtSInt32, a3, b3, Type::I32);

        let a4 = builder.const_(Literal::I32(10));
        let b4 = builder.const_(Literal::I32(20));
        let gt = builder.binary(BinaryOp::GtSInt32, a4, b4, Type::I32);

        let a5 = builder.const_(Literal::I32(10));
        let b5 = builder.const_(Literal::I32(20));
        let le = builder.binary(BinaryOp::LeSInt32, a5, b5, Type::I32);

        let a6 = builder.const_(Literal::I32(10));
        let b6 = builder.const_(Literal::I32(20));
        let ge = builder.binary(BinaryOp::GeSInt32, a6, b6, Type::I32);

        // f64 comparisons
        let c1 = builder.const_(Literal::F64(5.5));
        let d1 = builder.const_(Literal::F64(10.5));
        let f_eq = builder.binary(BinaryOp::EqFloat64, c1, d1, Type::I32);

        let c2 = builder.const_(Literal::F64(5.5));
        let d2 = builder.const_(Literal::F64(10.5));
        let f_lt = builder.binary(BinaryOp::LtFloat64, c2, d2, Type::I32);

        // Combine all results
        let r1 = builder.binary(BinaryOp::AddInt32, eq, ne, Type::I32);
        let r2 = builder.binary(BinaryOp::AddInt32, lt, gt, Type::I32);
        let r3 = builder.binary(BinaryOp::AddInt32, le, ge, Type::I32);
        let r4 = builder.binary(BinaryOp::AddInt32, f_eq, f_lt, Type::I32);

        let s1 = builder.binary(BinaryOp::AddInt32, r1, r2, Type::I32);
        let s2 = builder.binary(BinaryOp::AddInt32, r3, r4, Type::I32);
        let body = builder.binary(BinaryOp::AddInt32, s1, s2, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_memory_operations_sequence() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Complex memory operation sequence
        let addr1 = builder.const_(Literal::I32(0));
        let val1 = builder.load(4, false, 0, 2, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(4));
        let val2 = builder.load(4, false, 0, 2, addr2, Type::I32);

        let sum = builder.binary(BinaryOp::AddInt32, val1, val2, Type::I32);

        let store_addr = builder.const_(Literal::I32(8));
        let store1 = builder.store(4, 0, 2, store_addr, sum);

        let load_back_addr = builder.const_(Literal::I32(8));
        let loaded_back = builder.load(4, false, 0, 2, load_back_addr, Type::I32);

        let body = builder.block(
            None,
            BumpVec::from_iter_in([store1, loaded_back], &bump),
            Type::I32,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_all_conversion_paths() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Test all conversion paths in one function
        let i32_val = builder.const_(Literal::I32(42));

        // i32 -> f32 -> f64 -> i64 -> i32
        let i32_to_f32 = builder.unary(UnaryOp::ConvertSInt32ToFloat32, i32_val, Type::F32);
        let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_sum = builder.binary(BinaryOp::AddFloat32, i32_to_f32, f32_val, Type::F32);

        let f32_to_i64 = builder.unary(UnaryOp::TruncSFloat32ToInt64, f32_sum, Type::I64);
        let i64_doubled = builder.binary(
            BinaryOp::MulInt64,
            f32_to_i64,
            builder.const_(Literal::I64(2)),
            Type::I64,
        );

        let i64_to_f64 = builder.unary(UnaryOp::ConvertSInt64ToFloat64, i64_doubled, Type::F64);
        let f64_val = builder.const_(Literal::F64(1.5));
        let f64_div = builder.binary(BinaryOp::DivFloat64, i64_to_f64, f64_val, Type::F64);

        let body = builder.unary(UnaryOp::TruncSFloat64ToInt32, f64_div, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_loop_with_complex_body() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Create a loop with complex operations inside
        let i32_val = builder.const_(Literal::I32(10));
        let i32_mul = builder.binary(
            BinaryOp::MulInt32,
            i32_val,
            builder.const_(Literal::I32(2)),
            Type::I32,
        );

        let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_sqrt = builder.unary(UnaryOp::SqrtFloat32, f32_val, Type::F32);
        let f32_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, f32_sqrt, Type::I32);

        let combined = builder.binary(BinaryOp::AddInt32, i32_mul, f32_to_i32, Type::I32);

        let loop_body = builder.loop_(Some("loop"), combined, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(loop_body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_select_with_all_types() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Test select with i32
        let cond1 = builder.const_(Literal::I32(1));
        let i32_true = builder.const_(Literal::I32(100));
        let i32_false = builder.const_(Literal::I32(200));
        let select_i32 = builder.select(cond1, i32_true, i32_false, Type::I32);

        // Test select with i64
        let cond2 = builder.binary(
            BinaryOp::GtSInt32,
            select_i32,
            builder.const_(Literal::I32(150)),
            Type::I32,
        );
        let i64_true = builder.const_(Literal::I64(1000));
        let i64_false = builder.const_(Literal::I64(2000));
        let select_i64 = builder.select(cond2, i64_true, i64_false, Type::I64);

        // Test select with f32
        let cond3 = builder.binary(
            BinaryOp::LtSInt64,
            select_i64,
            builder.const_(Literal::I64(1500)),
            Type::I32,
        );
        let f32_true = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_false = builder.const_(Literal::F32(2.71));
        let select_f32 = builder.select(cond3, f32_true, f32_false, Type::F32);

        // Convert back to i32
        let body = builder.unary(UnaryOp::TruncSFloat32ToInt32, select_f32, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_stress_all_bitwise_operations() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // All i32 bitwise ops
        let a1 = builder.const_(Literal::I32(0xAAAA5555u32 as i32));
        let b1 = builder.const_(Literal::I32(0x5555AAAAu32 as i32));
        let and = builder.binary(BinaryOp::AndInt32, a1, b1, Type::I32);

        let a2 = builder.const_(Literal::I32(0xAAAA5555u32 as i32));
        let b2 = builder.const_(Literal::I32(0x5555AAAAu32 as i32));
        let or = builder.binary(BinaryOp::OrInt32, a2, b2, Type::I32);

        let a3 = builder.const_(Literal::I32(0xAAAA5555u32 as i32));
        let b3 = builder.const_(Literal::I32(0x5555AAAAu32 as i32));
        let xor = builder.binary(BinaryOp::XorInt32, a3, b3, Type::I32);

        let shift_val1 = builder.const_(Literal::I32(4));
        let shl = builder.binary(BinaryOp::ShlInt32, and, shift_val1, Type::I32);

        let shift_val2 = builder.const_(Literal::I32(4));
        let shr_s = builder.binary(BinaryOp::ShrSInt32, or, shift_val2, Type::I32);

        let shift_val3 = builder.const_(Literal::I32(4));
        let shr_u = builder.binary(BinaryOp::ShrUInt32, xor, shift_val3, Type::I32);

        let rot_val1 = builder.const_(Literal::I32(8));
        let rotl = builder.binary(BinaryOp::RotLInt32, shl, rot_val1, Type::I32);

        let rot_val2 = builder.const_(Literal::I32(8));
        let rotr = builder.binary(BinaryOp::RotRInt32, shr_s, rot_val2, Type::I32);

        let r1 = builder.binary(BinaryOp::XorInt32, rotl, rotr, Type::I32);
        let body = builder.binary(BinaryOp::XorInt32, r1, shr_u, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }

    #[test]
    fn test_maximum_expression_complexity() {
        use crate::wasm::binary_reader::BinaryReader;
        use crate::wasm::binary_writer::BinaryWriter;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Combine: nested blocks + loops + ifs + all operation types + memory ops + conversions
        let i32_start = builder.const_(Literal::I32(1));
        let i64_val = builder.const_(Literal::I64(100));

        // Memory operation
        let addr = builder.const_(Literal::I32(0));
        let stored_val = builder.binary(
            BinaryOp::AddInt32,
            i32_start,
            builder.const_(Literal::I32(10)),
            Type::I32,
        );
        let store_op = builder.store(4, 0, 2, addr, stored_val);

        // Load and process
        let load_addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(4, false, 0, 2, load_addr, Type::I32);

        // Float operations
        let loaded_f32 = builder.unary(UnaryOp::ConvertSInt32ToFloat32, loaded, Type::F32);
        let f32_processed = builder.unary(UnaryOp::SqrtFloat32, loaded_f32, Type::F32);
        let back_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, f32_processed, Type::I32);

        // Comparison and select
        let condition = builder.binary(
            BinaryOp::GtSInt32,
            back_to_i32,
            builder.const_(Literal::I32(5)),
            Type::I32,
        );
        let selected = builder.select(
            condition,
            i64_val,
            builder.const_(Literal::I64(999)),
            Type::I64,
        );

        // Bitwise operations
        let i64_as_i32 = builder.unary(
            UnaryOp::TruncSFloat64ToInt32,
            builder.unary(UnaryOp::ConvertSInt64ToFloat64, selected, Type::F64),
            Type::I32,
        );
        let rotated = builder.binary(
            BinaryOp::RotLInt32,
            i64_as_i32,
            builder.const_(Literal::I32(4)),
            Type::I32,
        );

        // Final block
        let body = builder.block(
            None,
            BumpVec::from_iter_in([store_op, rotated], &bump),
            Type::I32,
        );

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new(&bump, bytes);
        let parsed = reader.parse_module().expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
    }
}

#[cfg(test)]
mod allocator_stress_test;
