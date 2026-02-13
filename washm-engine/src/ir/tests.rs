use super::*;
use crate::analysis::validation::Validator;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::*;
use crate::ir::module::*;
use crate::ir::ops::*;
use crate::ir::visitor::Visitor;

#[test]
fn test_type_name() {
    assert_eq!(Type::I32.to_string(), "i32");
    assert_eq!(Type::F64.to_string(), "f64");
}

#[test]
fn test_type_equality() {
    assert_eq!(Type::I32, Type::I32);
    assert_ne!(Type::I32, Type::I64);
}

#[test]
fn test_ref_types() {
    assert!(Type::FUNCREF.is_ref());
    assert!(Type::FUNCREF.is_nullable());
    assert!(!Type::I32.is_ref());
    assert!(!Type::I32.is_nullable());

    let func_heap_type = Type::FUNCREF.get_heap_type().unwrap();
    assert_eq!(func_heap_type, HeapType::FUNC);
}

#[test]
fn test_signature() {
    let sig = Signature::new(Type::I32, Type::F64);
    assert_eq!(sig.params, Type::I32);
    assert_eq!(sig.results, Type::F64);
}

#[test]
fn test_validation_failure() {
    let mut module = Module::new();

    // Create mismatched binary op: i32 + f32
    let left = module.expressions.alloc(Expression::new(
        ExpressionKind::Const(Literal::I32(1)),
        Type::I32,
    ));
    let right = module.expressions.alloc(Expression::new(
        ExpressionKind::Const(Literal::F32(2.0)),
        Type::F32,
    ));

    let binary_expr = module.expressions.alloc(Expression::new(
        ExpressionKind::Binary {
            op: BinaryOp::AddInt32, // Note: using int32 add
            left,
            right,
        },
        Type::I32, // Result type claimed to be i32
    ));

    let func = Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        binary_expr,
    );
    module.add_function(func);

    let validator = Validator::new(&module);
    let (valid, errors) = validator.validate();

    assert!(!valid, "Validation should fail for mismatched types");
    assert!(!errors.is_empty());
    assert!(errors[0].contains("Binary op AddInt32 operands type mismatch"));
}

#[test]
fn test_global_validation() {
    let mut module = Module::new();
    let _builder = IrBuilder::new(&mut module.expressions);

    // 1. Test GlobalSet on immutable global (g0)
    {
        let mut module = Module::new();
        let g0_id = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            module.globals.alloc(crate::ir::module::Global {
                name: "g0".to_string(),
                kind: crate::ir::module::GlobalKind::Local {
                    init: builder.const_(Literal::I32(0)),
                },
                type_: Type::I32,
                mutable: false,
            })
        };

        let set_immutable = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let val = builder.const_(Literal::I32(42));
            builder.global_set(g0_id, val)
        };

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            set_immutable,
        );
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Setting immutable global should fail");
        assert!(errors.iter().any(|e| e.contains("is immutable")));
    }

    // 2. Test GlobalSet type mismatch
    {
        let mut module = Module::new();
        let g1_id = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            module.globals.alloc(crate::ir::module::Global {
                name: "g0".to_string(),
                kind: crate::ir::module::GlobalKind::Local {
                    init: builder.const_(Literal::I32(0)),
                },
                type_: Type::I32,
                mutable: false,
            });
            module.globals.alloc(crate::ir::module::Global {
                name: "g1".to_string(),
                kind: crate::ir::module::GlobalKind::Local {
                    init: builder.const_(Literal::F32(0.0)),
                },
                type_: Type::F32,
                mutable: true,
            })
        };

        let set_mismatch = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let val = builder.const_(Literal::I32(42)); // i32
            builder.global_set(g1_id, val)
        };

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            set_mismatch,
        );
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "GlobalSet type mismatch should fail");
        assert!(errors
            .iter()
            .any(|e| e.contains("Value type i32 does not match global type f32")));
    }

    // 3. Test GlobalGet out of bounds
    {
        let mut module = Module::new();
        let get_oob = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let oob_id: GlobalId = GlobalId::from_raw_parts(99, 0);
            builder.global_get(oob_id, Type::I32)
        };

        let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], get_oob);
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "OOB GlobalGet should fail");
        assert!(errors.iter().any(|e| e.contains("Invalid ID")));
    }
}

#[test]
fn test_ir_construction() {
    let mut module = Module::new();
    let (const_expr, block) = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        let const_expr = builder.const_(Literal::I32(42));
        let list = vec![const_expr];
        let block = builder.block("my_block", list, Type::I32);
        (const_expr, block)
    };

    match module.expressions[const_expr].kind {
        ExpressionKind::Const(Literal::I32(42)) => (),
        _ => panic!("Expected Const(42)"),
    }
    assert_eq!(module.expressions[const_expr].type_, Type::I32);

    if let ExpressionKind::Block { name, list } = &module.expressions[block].kind {
        assert_eq!(*name, Some("my_block".to_string()));
        assert_eq!(list.len(), 1);
    } else {
        panic!("Expected Block");
    }
}

#[test]
fn test_binary_op() {
    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

    let left = builder.const_(Literal::I32(10));
    let right = builder.const_(Literal::I32(32));

    let add = builder.binary(BinaryOp::AddInt32, left, right, Type::I32);

    if let ExpressionKind::Binary { op, left, right } = &module.expressions[add].kind {
        assert_eq!(*op, BinaryOp::AddInt32);
        assert_eq!(module.expressions[*left].type_, Type::I32);
        assert_eq!(module.expressions[*right].type_, Type::I32);
    } else {
        panic!("Expected Binary");
    }
}

struct CountVisitor {
    count: usize,
}

impl Visitor for CountVisitor {
    fn visit_expression(
        &mut self,
        _expr: &mut ExpressionId,
        _arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.count += 1;
    }
}

#[test]
fn test_visitor() {
    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

    let c1 = builder.const_(Literal::I32(1));
    let c2 = builder.const_(Literal::I32(2));
    let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

    let list = vec![add];
    let mut block = builder.block("", list, Type::I32);

    let mut v = CountVisitor { count: 0 };
    v.visit(&mut block, &mut module.expressions);

    // Block (1) -> Add (1) -> Const (1) + Const (1) = 4 expressions
    assert_eq!(v.count, 4);
}

#[test]
fn test_module_construction() {
    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

    // fn add_one(x: i32) -> i32
    let local_get = builder.local_get(crate::ir::LocalId::from_u32(0), Type::I32);
    let const_1 = builder.const_(Literal::I32(1));
    let add = builder.binary(BinaryOp::AddInt32, local_get, const_1, Type::I32);

    let func = Function::new("add_one".to_string(), Type::I32, Type::I32, vec![], add);
    module.add_function(func);

    assert!(module.get_function_by_name("add_one").is_some());

    let (_, f) = module.get_function_by_name("add_one").unwrap();
    if let Some(body) = f.body() {
        if let ExpressionKind::Binary { op, .. } = module.expressions[body].kind {
            assert_eq!(op, BinaryOp::AddInt32);
        } else {
            panic!("Expected Binary");
        }
    }
}

#[test]
fn test_export_validation() {
    use crate::ir::module::{ExportKind, Global, MemoryLimits};

    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);
    let _nop = builder.nop();

    // Setup a basic valid module components
    let global_init = builder.const_(Literal::I32(0));
    let _global = Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Local { init: global_init },
    };

    let memory = MemoryLimits {
        initial: 1,
        maximum: None,
    };

    // 1. Valid exports
    {
        let mut module = Module::new();
        let (nop, g0_init) = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            (builder.nop(), builder.const_(Literal::I32(0)))
        };
        let g0_id = module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: g0_init },
        });
        let func_valid = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], nop);
        module.add_function(func_valid);
        module.add_memory(Memory {
            name: "".to_string(),
            kind: MemoryKind::Local,
            limits: memory.clone(),
        });
        module.add_export(
            "exp_func".to_string(),
            ExportKind::Function(FuncId::from_raw_parts(0, 0)),
        );
        module.add_export("exp_glob".to_string(), ExportKind::Global(g0_id));
        module.add_export(
            "exp_mem".to_string(),
            ExportKind::Memory(MemoryId::from_raw_parts(0, 0)),
        );

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Valid exports failed: {:?}", errors);
    }

    // 2. Duplicate export name
    {
        let mut module = Module::new();
        module.add_export(
            "same".to_string(),
            ExportKind::Function(FuncId::from_raw_parts(0, 0)),
        );
        module.add_export(
            "same".to_string(),
            ExportKind::Function(FuncId::from_raw_parts(0, 0)),
        );

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid);
        assert!(errors.iter().any(|e| e.contains("Duplicate export name")));
    }

    // 3. Function OOB
    {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);
        let nop = builder.nop();
        let func_valid = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], nop);
        module.add_function(func_valid);
        module.add_export(
            "f1".to_string(),
            ExportKind::Function(FuncId::from_raw_parts(99, 0)),
        );
        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid);
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // 4. Global OOB
    {
        let mut module = Module::new();
        let oob_id: GlobalId = GlobalId::from_raw_parts(99, 0);
        module.add_export("g0".to_string(), ExportKind::Global(oob_id));

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid);
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // 5. Memory OOB / No Memory
    {
        let mut module = Module::new();
        module.add_export(
            "m0".to_string(),
            ExportKind::Memory(MemoryId::from_raw_parts(0, 0)),
        );
        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid);
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }
}

#[test]
fn test_exports_roundtrip() {
    use crate::ir::module::{ExportKind, Global, MemoryLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let (nop, init_expr) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (builder.nop(), builder.const_(Literal::I32(123)))
    };

    let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], nop);
    module.add_function(func);
    let g0_id = module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: crate::ir::module::GlobalKind::Local { init: init_expr },
    });
    module.add_memory(Memory {
        name: "".to_string(),
        limits: MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: MemoryKind::Local,
    });

    module.add_export(
        "test_func".to_string(),
        ExportKind::Function(FuncId::from_raw_parts(0, 0)),
    );
    module.add_export("test_glob".to_string(), ExportKind::Global(g0_id.cast()));
    module.add_export(
        "test_mem".to_string(),
        ExportKind::Memory(MemoryId::from_raw_parts(0, 0)),
    );

    // Write
    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("write failed");

    // Read
    let mut reader = BinaryReader::new();
    let read_module = reader.parse_module(&bytes).expect("parse failed");

    // Verify
    assert_eq!(read_module.exports.len(), 3);

    let func_exp = read_module
        .exports
        .iter()
        .find(|e| e.name == "test_func")
        .unwrap();
    assert_eq!(
        func_exp.kind,
        ExportKind::Function(FuncId::from_raw_parts(0, 0))
    );

    let glob_exp = read_module
        .exports
        .iter()
        .find(|e| e.name == "test_glob")
        .unwrap();
    let g0_id = read_module.globals.ids().next().unwrap().cast();
    assert_eq!(glob_exp.kind, ExportKind::Global(g0_id));

    let mem_exp = read_module
        .exports
        .iter()
        .find(|e| e.name == "test_mem")
        .unwrap();
    assert_eq!(
        mem_exp.kind,
        ExportKind::Memory(MemoryId::from_raw_parts(0, 0))
    );
}

#[test]
fn test_imports_roundtrip() {
    use crate::ir::module::{Import, ImportKind, MemoryLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // 1. Function Import (param: i32, result: none)
    module.add_import(Import {
        module: "env".to_string(),
        name: "log".to_string(),
        kind: ImportKind::Function(Type::I32, Type::NONE),
    });

    // 2. Global Import (i32, immutable)
    module.add_global(Global {
        name: "limit".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Import {
            module: "env".to_string(),
            field: "limit".to_string(),
        },
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
    let mut reader = BinaryReader::new();
    let read_module = reader.parse_module(&bytes).expect("parse failed");

    assert_eq!(read_module.functions.len(), 1);
    assert_eq!(read_module.globals.len(), 1);
    assert_eq!(read_module.memories.len(), 1);

    let log_func_id = read_module.functions.ids().next().unwrap();
    let log_func = &read_module.functions[log_func_id];
    let log_import = if let crate::ir::module::FunctionKind::Import(import) = &log_func.kind {
        import
    } else {
        panic!("Expected function import");
    };
    assert_eq!(log_import.module, "env");
    assert_eq!(log_import.name, "log");
    assert_eq!(log_func.params, Type::I32);
    assert_eq!(log_func.results, Type::NONE);

    let mem_id = read_module.memories.ids().next().unwrap();
    let memory_import = &read_module.memories[mem_id];
    if let crate::ir::module::MemoryKind::Import { module, field } = &memory_import.kind {
        assert_eq!(module, "env");
        assert_eq!(field, "memory");
    } else {
        panic!("Expected memory import");
    }

    let (_, global_import) = read_module.globals.iter().next().unwrap();
    assert_eq!(global_import.name, "limit");
    if let GlobalKind::Import { module, field } = &global_import.kind {
        assert_eq!(module, "env");
        assert_eq!(field, "limit");
        assert_eq!(global_import.type_, Type::I32);
        assert!(!global_import.mutable);
    } else {
        panic!("Expected global import");
    }
}

#[test]
fn test_import_validation() {
    use crate::ir::module::Global;

    let mut module = Module::new();

    // Import Global: i32, immutable
    let g_imp_id = module.add_global(Global {
        name: "g_imp".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Import {
            module: "env".to_string(),
            field: "g_imp".to_string(),
        },
    });

    // Define Global: f32, mutable
    let init_expr = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::F32(1.0))
    };
    let g_def_id = module.add_global(Global {
        name: "g_def".to_string(),
        type_: Type::F32,
        mutable: true,
        kind: GlobalKind::Local { init: init_expr },
    });

    // Test 1: Get imported global - Valid
    {
        let get_imp = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.global_get(g_imp_id, Type::I32)
        };
        let func = Function::new("test1".to_string(), Type::NONE, Type::I32, vec![], get_imp);
        module.functions.push(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Get imported global failed: {:?}", errors);
        module.functions.pop();
    }

    // Test 2: Get defined global - Valid
    {
        let get_def = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.global_get(g_def_id, Type::F32)
        };
        let func = Function::new("test2".to_string(), Type::NONE, Type::F32, vec![], get_def);
        module.functions.push(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Get defined global failed: {:?}", errors);
        module.functions.pop();
    }

    // Test 3: Get OOB global - Invalid
    {
        let get_oob = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.global_get(crate::ir::GlobalId::from_raw_parts(2, 0).cast(), Type::I32)
        };
        let func = Function::new("test3".to_string(), Type::NONE, Type::I32, vec![], get_oob);
        module.functions.push(func);

        let validator = Validator::new(&module);
        let (valid, _) = validator.validate();
        assert!(!valid, "OOB global get should fail");
        module.functions.pop();
    }
}

#[test]
fn test_data_section_roundtrip() {
    use crate::ir::module::DataSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let (offset1, offset2) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(100)),
        )
    };

    // Add memory
    module.set_memory(1, Some(10));

    // Data segment 1: "Hello" at offset 0
    module.add_data_segment(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset: offset1,
        data: b"Hello".to_vec(),
    });

    // Data segment 2: "World" at offset 100
    module.add_data_segment(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset: offset2,
        data: b"World".to_vec(),
    });

    // Write
    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("write failed");

    // Read
    let mut reader = BinaryReader::new();
    let read_module = reader.parse_module(&bytes).expect("parse failed");

    // Verify
    assert_eq!(read_module.data.len(), 2);

    let seg1 = &read_module.data[0];
    assert_eq!(seg1.memory_index, MemoryId::from_raw_parts(0, 0));
    assert_eq!(seg1.data, b"Hello");
    if let ExpressionKind::Const(Literal::I32(val)) = read_module.expressions[seg1.offset].kind {
        assert_eq!(val, 0);
    } else {
        panic!("Expected const offset");
    }

    let seg2 = &read_module.data[1];
    assert_eq!(seg2.memory_index, MemoryId::from_raw_parts(0, 0));
    assert_eq!(seg2.data, b"World");
    if let ExpressionKind::Const(Literal::I32(val)) = read_module.expressions[seg2.offset].kind {
        assert_eq!(val, 100);
    } else {
        panic!("Expected const offset");
    }
}

#[test]
fn test_data_section_validation() {
    use crate::ir::module::DataSegment;

    // Test 1: Valid data segment
    {
        let mut module = Module::new();
        module.set_memory(1, None);

        let offset = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(0))
        };
        module.add_data_segment(DataSegment {
            memory_index: MemoryId::from_raw_parts(0, 0),
            offset,
            data: b"test".to_vec(),
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Valid data segment failed: {:?}", errors);
    }

    // Test 2: Data segment without memory
    {
        let mut module = Module::new();
        // No memory defined

        let offset = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(0))
        };
        module.add_data_segment(DataSegment {
            memory_index: MemoryId::from_raw_parts(0, 0),
            offset,
            data: b"test".to_vec(),
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Data without memory should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // Test 3: Invalid memory index
    {
        let mut module = Module::new();
        module.set_memory(1, None);

        let offset = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(0))
        };
        module.add_data_segment(DataSegment {
            memory_index: 1.into(), // Invalid: only 0 allowed
            offset,
            data: b"test".to_vec(),
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Invalid memory index should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }
}

#[test]
fn test_start_section_roundtrip() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Add a function to be the start function
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let func = Function::new("init".to_string(), Type::NONE, Type::NONE, vec![], body);
    module.add_function(func);

    // Set as start function (index 0)
    module.set_start(FuncId::from_raw_parts(0, 0));

    // Write
    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("write failed");

    // Read
    let mut reader = BinaryReader::new();
    let read_module = reader.parse_module(&bytes).expect("parse failed");

    // Verify
    assert_eq!(read_module.start, Some(FuncId::from_raw_parts(0, 0)));
}

#[test]
fn test_start_section_validation() {
    // Test 1: Valid start function (no params, no results)
    {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let const_expr = builder.const_(Literal::I32(0));
            builder.drop(const_expr) // Drop the i32 to get Type::NONE
        };
        let func = Function::new("start".to_string(), Type::NONE, Type::NONE, vec![], body);
        module.add_function(func);
        module.set_start(FuncId::from_raw_parts(0, 0));

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Valid start function failed: {:?}", errors);
    }

    // Test 2: Start function out of bounds
    {
        let mut module = Module::new();
        module.set_start(99.into()); // No functions exist

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "OOB start function should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // Test 3: Start function with parameters (invalid)
    {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.local_get(0, Type::I32)
        };
        let func = Function::new(
            "bad_start".to_string(),
            Type::I32, // Has parameter
            Type::NONE,
            vec![],
            body,
        );
        module.add_function(func);
        module.set_start(FuncId::from_raw_parts(0, 0));

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Start with params should fail");
        assert!(errors.iter().any(|e| e.contains("no parameters")));
    }

    // Test 4: Start function with results (invalid)
    {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(42))
        };
        let func = Function::new(
            "bad_start".to_string(),
            Type::NONE,
            Type::I32, // Has result
            vec![],
            body,
        );
        module.add_function(func);
        module.set_start(FuncId::from_raw_parts(0, 0));

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Start with results should fail");
        assert!(errors.iter().any(|e| e.contains("no results")));
    }
}

#[test]
fn test_table_and_element_roundtrip() {
    use crate::ir::module::ElementSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Add functions that will be referenced in the element segment
    let (nop, offset) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (builder.nop(), builder.const_(Literal::I32(0)))
    };
    let func1 = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], nop);
    let func2 = Function::new("f1".to_string(), Type::NONE, Type::NONE, vec![], nop);
    module.add_function(func1);
    module.add_function(func2);

    // Add table
    module.set_table(10, Some(20));

    // Add element segment: initialize table[0..2] with functions [0, 1]
    module.add_element_segment(ElementSegment {
        table_index: TableId::from_raw_parts(0, 0),
        offset,
        func_indices: vec![FuncId::from_raw_parts(0, 0), 1.into()],
    });

    // Write
    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("write failed");

    // Read
    let mut reader = BinaryReader::new();
    let read_module = reader.parse_module(&bytes).expect("parse failed");

    // Verify table
    assert!(!read_module.tables.is_empty());
    let table = read_module.tables.unwrap_first();
    assert_eq!(table.limits.element_type, Type::FUNCREF);
    assert_eq!(table.limits.initial, 10);
    assert_eq!(table.limits.maximum, Some(20));

    // Verify element segments
    assert_eq!(read_module.elements.len(), 1);
    let elem = &read_module.elements[0];
    assert_eq!(elem.table_index, TableId::from_raw_parts(0, 0));
    assert_eq!(
        elem.func_indices,
        vec![FuncId::from_raw_parts(0, 0), 1.into()]
    );
    if let ExpressionKind::Const(Literal::I32(val)) = read_module.expressions[elem.offset].kind {
        assert_eq!(val, 0);
    } else {
        panic!("Expected const offset");
    }
}

#[test]
fn test_table_and_element_validation() {
    use crate::ir::module::ElementSegment;

    // Test 1: Valid table and element segment
    {
        let mut module = Module::new();

        let (nop, offset) = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            (builder.nop(), builder.const_(Literal::I32(0)))
        };
        let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], nop);
        module.add_function(func);

        module.add_table(Table {
            name: "".to_string(),
            limits: TableLimits {
                initial: 0,
                maximum: None,
                element_type: Type::FUNCREF,
            },
            kind: TableKind::Local,
        });

        module.add_element_segment(ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![FuncId::from_raw_parts(0, 0)],
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Valid table/element failed: {:?}", errors);
    }

    // Test 2: Element segment without table
    {
        let mut module = Module::new();

        let (nop, offset) = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            (builder.nop(), builder.const_(Literal::I32(0)))
        };
        let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], nop);
        module.add_function(func);

        module.add_element_segment(ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![FuncId::from_raw_parts(0, 0)],
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Element without table should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // Test 3: Element with invalid function index
    {
        let mut module = Module::new();

        module.add_table(Table {
            name: "".to_string(),
            limits: TableLimits {
                initial: 0,
                maximum: None,
                element_type: Type::FUNCREF,
            },
            kind: TableKind::Local,
        });

        let offset = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(0))
        };
        module.add_element_segment(ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![99.into()], // Out of bounds
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Element with OOB function should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // Test 4: Invalid table index in element segment
    {
        let mut module = Module::new();

        let (nop, offset) = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            (builder.nop(), builder.const_(Literal::I32(0)))
        };
        let func = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], nop);
        module.add_function(func);

        module.add_table(Table {
            name: "".to_string(),
            limits: TableLimits {
                initial: 0,
                maximum: None,
                element_type: Type::FUNCREF,
            },
            kind: TableKind::Local,
        });

        module.add_element_segment(ElementSegment {
            table_index: 1.into(), // Invalid: only 0 allowed
            offset,
            func_indices: vec![FuncId::from_raw_parts(0, 0)],
        });

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Invalid table index should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }
}

#[test]
fn test_type_section_roundtrip() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test 1: Empty types
    {
        let module = Module::new();

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

        assert_eq!(parsed.types.len(), 0);
    }

    // Test 2: Single type (i32) -> (i32)
    {
        let mut module = Module::new();
        module.add_type(Type::I32, Type::I32);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

        assert_eq!(parsed.types.len(), 1);
        assert_eq!(parsed.types[0].params, Type::I32);
        assert_eq!(parsed.types[0].results, Type::I32);
    }

    // Test 3: Multiple types
    {
        let mut module = Module::new();
        module.add_type(Type::I32, Type::I32);
        module.add_type(Type::I32, Type::NONE);
        module.add_type(Type::NONE, Type::F64);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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
            let mut module = Module::new();
            module.add_type(*param_type, *result_type);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|_| panic!("Failed to write {} -> {}", param_name, result_name));

            let mut reader = BinaryReader::new();
            let parsed = reader
                .parse_module(&bytes)
                .unwrap_or_else(|_| panic!("Failed to parse {} -> {}", param_name, result_name));

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
        let mut module = Module::new();
        module.add_type(Type::NONE, Type::NONE);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

        assert_eq!(parsed.types.len(), 1);
        assert_eq!(parsed.types[0].params, Type::NONE);
        assert_eq!(parsed.types[0].results, Type::NONE);
    }

    // Test 2: () -> (i32)
    {
        let mut module = Module::new();
        module.add_type(Type::NONE, Type::I32);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

        assert_eq!(parsed.types.len(), 1);
        assert_eq!(parsed.types[0].params, Type::NONE);
        assert_eq!(parsed.types[0].results, Type::I32);
    }

    // Test 3: (f64) -> ()
    {
        let mut module = Module::new();
        module.add_type(Type::F64, Type::NONE);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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
            let mut module = Module::new();
            module.add_type(*ref_type, Type::NONE);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|_| panic!("Failed to write {} -> ()", name));

            let mut reader = BinaryReader::new();
            let parsed = reader
                .parse_module(&bytes)
                .unwrap_or_else(|_| panic!("Failed to parse {} -> ()", name));

            assert_eq!(parsed.types.len(), 1);
            assert_eq!(parsed.types[0].params, *ref_type);
            assert_eq!(parsed.types[0].results, Type::NONE);
        }

        // Test: () -> (ref)
        {
            let mut module = Module::new();
            module.add_type(Type::NONE, *ref_type);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|_| panic!("Failed to write () -> {}", name));

            let mut reader = BinaryReader::new();
            let parsed = reader
                .parse_module(&bytes)
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
    let mut module = Module::new();
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

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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

    let mut module = Module::new();

    // Add explicit types
    module.add_type(Type::I32, Type::I32); // type 0
    module.add_type(Type::F64, Type::F64); // type 1

    // Add functions that should reference these types
    let (body1, body2) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (
            builder.const_(Literal::I32(42)),
            builder.const_(Literal::F64(1.23)),
        )
    };

    let func1 = Function::new("f1".to_string(), Type::I32, Type::I32, vec![], body1);
    module.add_function(func1);

    let func2 = Function::new("f2".to_string(), Type::F64, Type::F64, vec![], body2);
    module.add_function(func2);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    // Should have at least the 2 explicit types (writer may add more if needed)
    assert!(parsed.types.len() >= 2, "Should have at least 2 types");

    // Functions should be parsed
    assert_eq!(parsed.functions.len(), 2, "Should have 2 functions");
}

#[test]
fn test_type_section_with_imports() {
    use crate::ir::module::{Import, ImportKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

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

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    // Should have the explicit types
    assert_eq!(parsed.types.len(), 2, "Should have 2 types");

    assert_eq!(parsed.functions.len(), 2);
}

#[test]
fn test_type_section_deduplication() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Add three functions with the same signature
    for i in 0..3 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(i as i32))
        };
        let func = Function::new(format!("f{}", i), Type::I32, Type::I32, vec![], body);
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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

    let mut module = Module::new();

    // Add explicit types
    module.add_type(Type::I32, Type::I32); // type 0
    module.add_type(Type::F64, Type::F64); // type 1
    module.add_type(Type::NONE, Type::I64); // type 2

    // Create functions with explicit type indices
    let (body1, body2, body3) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (
            builder.const_(Literal::I32(42)),
            builder.const_(Literal::F64(std::f64::consts::PI)),
            builder.const_(Literal::I64(99)),
        )
    };
    let func1 = Function::with_type_idx(
        "f0".to_string(),
        0.into(),
        Type::I32,
        Type::I32,
        vec![],
        body1,
    );
    module.add_function(func1);

    let func2 = Function::with_type_idx(
        "f1".to_string(),
        1.into(),
        Type::F64,
        Type::F64,
        vec![],
        body2,
    );
    module.add_function(func2);

    let func3 = Function::with_type_idx(
        "f2".to_string(),
        2.into(),
        Type::NONE,
        Type::I64,
        vec![],
        body3,
    );
    module.add_function(func3);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 3);
    assert_eq!(parsed.functions.len(), 3);

    // Verify type indices are preserved
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].type_idx,
        Some(0.into())
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(1, 0)].type_idx,
        Some(1.into())
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(2, 0)].type_idx,
        Some(2.into())
    );
}

#[test]
fn test_function_section_mixed_type_specification() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_type(Type::I32, Type::I32); // type 0

    let (body1, body2) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (
            builder.const_(Literal::I32(1)),
            builder.const_(Literal::F64(2.0)),
        )
    };

    // Function with explicit type_idx
    let func1 = Function::with_type_idx(
        "explicit".to_string(),
        0.into(),
        Type::I32,
        Type::I32,
        vec![],
        body1,
    );
    module.add_function(func1);

    // Function without type_idx (will be inferred)
    let func2 = Function::new("inferred".to_string(), Type::F64, Type::F64, vec![], body2);
    module.add_function(func2);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
    // Both functions should have type_idx after round-trip
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .type_idx
        .is_some());
    assert!(parsed.functions[FuncId::from_raw_parts(1, 0)]
        .type_idx
        .is_some());
}

#[test]
fn test_function_section_many_functions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    for i in 0..50 {
        let (param_type, result_type) = match i % 4 {
            0 => (Type::I32, Type::I32),
            1 => (Type::F64, Type::F64),
            2 => (Type::NONE, Type::I32),
            _ => (Type::I64, Type::NONE),
        };

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            match result_type {
                t if t == Type::I32 => builder.const_(Literal::I32(i as i32)),
                t if t == Type::I64 => builder.const_(Literal::I64(i as i64)),
                t if t == Type::F64 => builder.const_(Literal::F64(i as f64)),
                _ => builder.nop(),
            }
        };

        let func = Function::new(format!("func_{}", i), param_type, result_type, vec![], body);
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 50, "Should have 50 functions");
    // All functions should have type indices
    for (i, func) in parsed.functions.iter().enumerate() {
        assert!(
            func.1.type_idx.is_some(),
            "Function {} should have type_idx",
            i
        );
    }
}

#[test]
fn test_function_section_validation_type_idx_bounds() {
    use crate::analysis::validation::Validator;

    // Test 1: Valid type_idx
    {
        let mut module = Module::new();
        module.add_type(Type::I32, Type::I32);

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(42))
        };
        let func = Function::with_type_idx(
            "valid".to_string(),
            0.into(),
            Type::I32,
            Type::I32,
            vec![],
            body,
        );
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(valid, "Valid type_idx should pass: {:?}", errors);
    }

    // Test 2: Out of bounds type_idx
    {
        let mut module = Module::new();
        module.add_type(Type::I32, Type::I32);

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(42))
        };
        let func = Function::with_type_idx(
            "oob".to_string(),
            99.into(), // Out of bounds
            Type::I32,
            Type::I32,
            vec![],
            body,
        );
        module.add_function(func);

        let validator = Validator::new(&module);
        let (valid, errors) = validator.validate();
        assert!(!valid, "Out of bounds type_idx should fail");
        assert!(errors.iter().any(|e| e.contains("does not exist")));
    }

    // Test 3: Type signature mismatch
    {
        let mut module = Module::new();
        module.add_type(Type::I32, Type::I32);

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::F64(std::f64::consts::PI))
        };
        let func = Function::with_type_idx(
            "mismatch".to_string(),
            0.into(),
            Type::F64, // Doesn't match type 0
            Type::F64,
            vec![],
            body,
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

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let func = Function::new("f".to_string(), Type::I32, Type::I32, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    // Writer should have inferred and created a type
    assert!(!parsed.types.is_empty(), "Should have inferred types");
    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_function_section_all_value_type_combinations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let value_types = [
        (Type::I32, Literal::I32(0)),
        (Type::I64, Literal::I64(0)),
        (Type::F32, Literal::F32(0.0)),
        (Type::F64, Literal::F64(0.0)),
    ];

    for (i, (param_type, _)) in value_types.iter().enumerate() {
        for (j, (result_type, lit)) in value_types.iter().enumerate() {
            let mut module = Module::new();

            let body = {
                let mut builder = IrBuilder::new(&mut module.expressions);
                builder.const_(lit.clone())
            };
            let func = Function::new(
                format!("f_{}_{}", i, j),
                *param_type,
                *result_type,
                vec![],
                body,
            );
            module.add_function(func);

            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .unwrap_or_else(|e| panic!("Failed to write param {} result {}: {:?}", i, j, e));

            let mut reader = BinaryReader::new();
            let parsed = reader
                .parse_module(&bytes)
                .unwrap_or_else(|e| panic!("Failed to parse param {} result {}: {:?}", i, j, e));

            assert_eq!(parsed.functions.len(), 1);
            assert_eq!(
                parsed.functions[FuncId::from_raw_parts(0, 0)].params,
                *param_type
            );
            assert_eq!(
                parsed.functions[FuncId::from_raw_parts(0, 0)].results,
                *result_type
            );
        }
    }
}

#[test]
fn test_function_section_empty() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test module with types but no functions
    let mut module = Module::new();
    module.add_type(Type::I32, Type::I32);
    module.add_type(Type::F64, Type::F64);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 2);
    assert_eq!(parsed.functions.len(), 0);
}

#[test]
fn test_function_section_with_locals() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

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
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(42))
        };
        let func = Function::new(
            format!("f_{}", desc),
            Type::NONE,
            Type::I32,
            locals.clone(),
            body,
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .unwrap_or_else(|e| panic!("Failed to write: {}: {:?}", desc, e));

        let mut reader = BinaryReader::new();
        let parsed = reader
            .parse_module(&bytes)
            .unwrap_or_else(|e| panic!("Failed to parse: {}: {:?}", desc, e));

        assert_eq!(
            parsed.functions.len(),
            1,
            "Function count mismatch: {}",
            desc
        );
        let func_id = parsed.functions.ids().next().unwrap();
        assert_eq!(
            parsed.functions[func_id].vars().len(),
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

    let mut module = Module::new();
    module.add_type(Type::I32, Type::I32); // type 0

    // Create 5 functions all using type 0
    for i in 0..5 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(i))
        };
        let func = Function::with_type_idx(
            format!("shared_{}", i),
            0.into(),
            Type::I32,
            Type::I32,
            vec![],
            body,
        );
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 1, "Should have 1 shared type");
    assert_eq!(parsed.functions.len(), 5);

    // All functions should reference type 0
    for (i, func) in parsed.functions.iter().enumerate() {
        assert_eq!(
            func.1.type_idx,
            Some(0.into()),
            "Function {} should reference type 0",
            i
        );
    }
}

#[test]
fn test_function_section_type_ordering() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

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

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(lit)
        };
        let func =
            Function::with_type_idx(format!("f{}", i).into(), i.into(), typ, typ, vec![], body);
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 4);

    // Verify type indices are preserved (reverse order: 3, 2, 1, 0)
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].type_idx,
        Some(3.into())
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(1, 0)].type_idx,
        Some(2.into())
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(2, 0)].type_idx,
        Some(1.into())
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(3, 0)].type_idx,
        Some(0.into())
    );
}

#[test]
fn test_function_section_with_imports() {
    use crate::ir::module::{Import, ImportKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

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
    let (body1, body2) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        (
            builder.const_(Literal::I32(1)),
            builder.const_(Literal::F64(2.0)),
        )
    };
    let func1 = Function::with_type_idx(
        "local1".to_string(),
        0.into(),
        Type::I32,
        Type::I32,
        vec![],
        body1,
    );
    module.add_function(func1);

    let func2 = Function::with_type_idx(
        "local2".to_string(),
        1.into(),
        Type::F64,
        Type::F64,
        vec![],
        body2,
    );
    module.add_function(func2);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    // Function space: imports come first, then defined functions
    assert_eq!(parsed.functions.len(), 4);
    assert_eq!(parsed.types.len(), 2);
}

#[test]
fn test_function_section_no_body() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_type(Type::I32, Type::I32);

    let nop = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    let func = Function::with_type_idx(
        "no_body".to_string(),
        0.into(),
        Type::I32,
        Type::I32,
        vec![],
        nop,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_function_section_reference_types() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let nop = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };

    // funcref parameter
    let func1 = Function::new(
        "takes_funcref".to_string(),
        Type::FUNCREF,
        Type::NONE,
        vec![],
        nop,
    );
    module.add_function(func1);

    // externref parameter
    let func2 = Function::new(
        "takes_externref".to_string(),
        Type::EXTERNREF,
        Type::NONE,
        vec![],
        nop,
    );
    module.add_function(func2);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
    // Use iterator to get IDs in order
    let mut ids = parsed.functions.ids();
    let id1 = ids.next().unwrap();
    let id2 = ids.next().unwrap();
    assert_eq!(parsed.functions[id1].params, Type::FUNCREF);
    assert_eq!(parsed.functions[id2].params, Type::EXTERNREF);
}

#[test]
fn test_function_section_type_inference_deduplication() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let _builder = IrBuilder::new(&mut module.expressions);

    // Add 10 functions with same signature (no explicit type_idx)
    for i in 0..10 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(i))
        };
        let func = Function::new(
            format!("inferred_{}", i),
            Type::I32,
            Type::I32,
            vec![],
            body,
        );
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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
            func.1.type_idx,
            Some(0.into()),
            "Function {} should reference type 0",
            i
        );
    }
}

#[test]
fn test_function_section_validation_empty_module() {
    use crate::analysis::validation::Validator;

    // Test validation with no functions
    let module = Module::new();
    let validator = Validator::new(&module);
    let (valid, errors) = validator.validate();
    assert!(valid, "Empty module should be valid: {:?}", errors);
}

#[test]
fn test_function_section_validation_no_type_idx() {
    use crate::analysis::validation::Validator;

    // Test validation of function without explicit type_idx
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let func = Function::new("f".to_string(), Type::I32, Type::I32, vec![], body);
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

    // Test with diverse function signatures
    let mut module = Module::new();

    let signatures = [
        (Type::NONE, Type::NONE, Literal::I32(0), "() -> ()"),
        (Type::I32, Type::I32, Literal::I32(1), "(i32) -> (i32)"),
        (Type::I64, Type::NONE, Literal::I32(2), "(i64) -> ()"),
        (Type::NONE, Type::F32, Literal::F32(3.0), "() -> (f32)"),
        (Type::F64, Type::I64, Literal::I64(4), "(f64) -> (i64)"),
    ];

    for (i, (param, result, lit, _desc)) in signatures.iter().enumerate() {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(lit.clone())
        };
        let func = Function::new(format!("f{}", i), *param, *result, vec![], body);
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

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

    // Test with many types to ensure large type indices work
    let mut module = Module::new();

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
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::F64(std::f64::consts::PI))
    };
    let func = Function::with_type_idx(
        "f199".to_string(),
        199.into(),
        Type::F64,
        Type::F64,
        vec![],
        body,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 200);
    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].type_idx,
        Some(199.into())
    );
}

#[test]
fn test_function_section_locals_all_types() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test function with locals of all basic types
    let mut module = Module::new();

    let locals = vec![Type::I32, Type::I64, Type::F32, Type::F64, Type::V128];

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    let func = Function::new(
        "all_locals".to_string(),
        Type::NONE,
        Type::I32,
        locals,
        body,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars().len(),
        5
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars()[0],
        Type::I32
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars()[1],
        Type::I64
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars()[2],
        Type::F32
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars()[3],
        Type::F64
    );
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars()[4],
        Type::V128
    );
}

// ========== Code Section (Section 10) Tests ==========

#[test]
fn test_code_section_simple_const() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let func = Function::new("const_i32".to_string(), Type::NONE, Type::I32, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .body()
        .is_some());
}

#[test]
fn test_code_section_all_const_types() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test each constant type
    let test_cases = [
        (Literal::I32(42), Type::I32),
        (Literal::I64(123456), Type::I64),
        (Literal::F32(std::f32::consts::PI), Type::F32),
        (Literal::F64(std::f64::consts::E), Type::F64),
    ];

    for (lit, typ) in &test_cases {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(lit.clone())
        };
        let func = Function::new("const_test".to_string(), Type::NONE, *typ, vec![], body);
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let mut reader = BinaryReader::new();
        let parsed = reader.parse_module(&bytes).expect("Failed to parse");

        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.functions[FuncId::from_raw_parts(0, 0)].results, *typ);
    }
}

#[test]
fn test_code_section_binary_ops() {
    use crate::ir::ops::BinaryOp;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test only the integer binary operations that are currently supported
    let test_ops = [
        (BinaryOp::AddInt32, "add_i32"),
        (BinaryOp::SubInt32, "sub_i32"),
        (BinaryOp::MulInt32, "mul_i32"),
        (BinaryOp::AddInt64, "add_i64"),
    ];

    for (op, name) in &test_ops {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let left = builder.const_(Literal::I32(10));
            let right = builder.const_(Literal::I32(20));
            builder.binary(*op, left, right, Type::I32)
        };

        let func = Function::new(
            format!("test_{}", name),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        );
        module.add_function(func);

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .unwrap_or_else(|e| panic!("Failed to write {}: {:?}", name, e));

        let mut reader = BinaryReader::new();
        let parsed = reader
            .parse_module(&bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", name, e));

        assert_eq!(parsed.functions.len(), 1);
    }
}

// Note: Unary operations not yet supported in binary writer
// #[test]
// fn test_code_section_unary_ops() { ... }

#[test]
fn test_code_section_nested_expressions() {
    use crate::ir::ops::BinaryOp;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test: (1 + 2) + (3 + 4)
    let mut module = Module::new();

    let result = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let add1 = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        let c3 = builder.const_(Literal::I32(3));
        let c4 = builder.const_(Literal::I32(4));
        let add2 = builder.binary(BinaryOp::AddInt32, c3, c4, Type::I32);

        builder.binary(BinaryOp::AddInt32, add1, add2, Type::I32)
    };

    let func = Function::new("nested".to_string(), Type::NONE, Type::I32, vec![], result);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .body()
        .is_some());
}

#[test]
fn test_code_section_local_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test local.get and local.set
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let val = builder.const_(Literal::I32(42));
        let set_local = builder.local_set(LocalId::from(0), val);
        let get_local = builder.local_get(LocalId::from(0), Type::I32);
        builder.block("", vec![set_local, get_local], Type::I32)
    };

    let func = Function::new(
        "locals".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32],
        body,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars().len(),
        1
    );
}

#[test]
fn test_code_section_control_flow_block() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test block control flow
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        builder.block("", vec![c1, c2], Type::I32)
    };

    let func = Function::new("block".to_string(), Type::NONE, Type::I32, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    let id = parsed.functions.ids().next().unwrap();
    if let Some(body) = parsed.functions[id].body() {
        if let ExpressionKind::Block { .. } = &parsed.expressions[body].kind {
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

    // Test loop control flow
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let loop_body = builder.const_(Literal::I32(1));
        builder.loop_("", loop_body, Type::I32)
    };

    let func = Function::new("loop".to_string(), Type::NONE, Type::I32, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    if let Some(body) = parsed.functions[FuncId::from_raw_parts(0, 0)].body() {
        match &parsed.expressions[body].kind {
            ExpressionKind::Loop { .. } => {
                // Success - loop parsed correctly
            }
            ExpressionKind::Block { list, .. } => {
                // Accept a block containing a Loop child (robust to parser wrapping)
                if list.iter().any(|&child| {
                    matches!(parsed.expressions[child].kind, ExpressionKind::Loop { .. })
                }) {
                    // Success - found loop inside block
                } else {
                    panic!("Expected Loop expression (no Loop child found in block)");
                }
            }
            _ => {
                panic!("Expected Loop expression");
            }
        }
    }
}

#[test]
fn test_code_section_nop() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test nop instruction
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    let func = Function::new("nop".to_string(), Type::NONE, Type::NONE, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    if let Some(body) = parsed.functions[FuncId::from_raw_parts(0, 0)].body() {
        if let ExpressionKind::Nop = &module.expressions[body].kind {
            // Success - nop parsed correctly
        } else {
            panic!("Expected Nop expression");
        }
    }
}

#[test]
fn test_code_section_multiple_functions_with_bodies() {
    use crate::ir::ops::BinaryOp;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test multiple functions with different body types
    let mut module = Module::new();

    // Function 1: constant
    let body1 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(1))
    };
    let func1 = Function::new("f1".to_string(), Type::NONE, Type::I32, vec![], body1);
    module.add_function(func1);

    // Function 2: binary operation
    let body2 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let c1 = builder.const_(Literal::I32(2));
        let c2 = builder.const_(Literal::I32(3));
        builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32)
    };
    let func2 = Function::new("f2".to_string(), Type::NONE, Type::I32, vec![], body2);
    module.add_function(func2);

    // Function 3: nop
    let body3 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    let func3 = Function::new("f3".to_string(), Type::NONE, Type::NONE, vec![], body3);
    module.add_function(func3);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 3);
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .body()
        .is_some());
    assert!(parsed.functions[FuncId::from_raw_parts(1, 0)]
        .body()
        .is_some());
    assert!(parsed.functions[FuncId::from_raw_parts(2, 0)]
        .body()
        .is_some());
}

#[test]
fn test_code_section_deep_nesting() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test deeply nested blocks (10 levels)
    let mut module = Module::new();

    let expr = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let mut expr = builder.const_(Literal::I32(42));
        for _ in 0..10 {
            expr = builder.block("", vec![expr], Type::I32);
        }
        expr
    };

    let func = Function::new("deep".to_string(), Type::NONE, Type::I32, vec![], expr);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .body()
        .is_some());
}

#[test]
fn test_code_section_global_operations() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test global.get
    let mut module = Module::new();

    let (_g0_id, body) = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Add a global
        let global_init = builder.const_(Literal::I32(100));
        let g0_id = module.globals.alloc(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: global_init },
        });

        // Function that gets the global
        let body = builder.global_get(g0_id, Type::I32);
        (g0_id, body)
    };

    let func = Function::new(
        "get_global".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(parsed.globals.len(), 1);
}

#[test]
fn test_code_section_empty_function_body() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test function with no body (None)
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    let func = Function::new("empty".to_string(), Type::NONE, Type::NONE, vec![], body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ========== Validation and Edge Case Tests ==========

#[test]
fn test_validation_type_index_out_of_bounds() {
    use crate::analysis::validation::Validator;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };

    // Add a function with invalid type_idx (no types defined)
    let mut func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], body);
    func.type_idx = Some(99.into());
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
    use crate::analysis::validation::Validator;

    let mut module = Module::new();

    // Add a type
    module.add_type(Type::I32, Type::I64);

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };

    // Add function with mismatched signature
    let mut func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], body);
    func.type_idx = Some(0.into());
    module.add_function(func);

    let validator = Validator::new(&module);
    let (valid, errors) = validator.validate();

    assert!(!valid, "Should fail validation with signature mismatch");
    assert!(!errors.is_empty(), "Should have validation errors");
}

#[test]
fn test_validation_valid_type_idx() {
    use crate::analysis::validation::Validator;

    let mut module = Module::new();

    // Add a type
    module.add_type(Type::NONE, Type::NONE);

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };

    // Add function with valid type_idx and matching signature
    let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], body);
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
    let mut module = Module::new();
    for i in 0..1000 {
        let params = if i % 2 == 0 { Type::I32 } else { Type::I64 };
        let results = if i % 3 == 0 { Type::F32 } else { Type::F64 };
        module.add_type(params, results);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 1000);
}

#[test]
fn test_roundtrip_maximum_functions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test with many functions (500)
    let mut module = Module::new();
    for i in 0..500 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        let func = Function::new(format!("func_{}", i), Type::NONE, Type::I32, vec![], body);
        module.add_function(func);
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 500);
}

#[test]
fn test_roundtrip_various_type_signatures() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test with various single-param single-result types
    let mut module = Module::new();

    // i32 -> i64
    module.add_type(Type::I32, Type::I64);

    // f32 -> f64
    module.add_type(Type::F32, Type::F64);

    // i64 -> i32
    module.add_type(Type::I64, Type::I32);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.types.len(), 3);
}

#[test]
fn test_roundtrip_empty_module() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test completely empty module
    let module = Module::new();

    let mut writer = BinaryWriter::new();
    let bytes = writer
        .write_module(&module)
        .expect("Failed to write empty module");

    let mut reader = BinaryReader::new();
    let parsed = reader
        .parse_module(&bytes)
        .expect("Failed to parse empty module");

    assert_eq!(parsed.types.len(), 0);
    assert_eq!(parsed.functions.len(), 0);
    assert_eq!(parsed.globals.len(), 0);
    assert!(parsed.tables.is_empty());
}

#[test]
fn test_roundtrip_mixed_function_types() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test functions with and without type_idx
    let mut module = Module::new();

    // Add a type
    module.add_type(Type::I32, Type::I64);

    // Function with explicit type_idx
    module.add_function(Function::new_for_test(Function {
        name: "test".to_string(),
        params: Type::I32,
        results: Type::I64,
        type_idx: Some(0.into()),
        kind: crate::ir::module::FunctionKind::Local {
            locals: IdArena::new(),
            param_ids: Vec::new(),
            var_ids: Vec::new(),
            body: crate::ir::expression::ExpressionId::new_null(),
        },
    }));

    // Function without type_idx (will be inferred)
    let untyped_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    module.add_function(Function::new(
        "untyped".to_string(),
        Type::F32,
        Type::F64,
        vec![],
        untyped_body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
    // Both functions should have type_idx after parsing
    assert!(parsed.functions.iter().next().unwrap().1.type_idx.is_some());
    assert!(parsed.functions.iter().nth(1).unwrap().1.type_idx.is_some());
}

#[test]
fn test_code_section_many_locals() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test function with many local variables (100)
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let mut vars = Vec::new();
    for i in 0..100 {
        vars.push(if i % 2 == 0 { Type::I32 } else { Type::I64 });
    }

    let func = Function::new("many_locals".to_string(), Type::NONE, Type::I32, vars, body);
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars().len(),
        100
    );
}

#[test]
fn test_code_section_function_with_params_and_locals() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test function with parameters and local variables
    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(1))
    };
    let func = Function::new(
        "params_and_locals".to_string(),
        Type::I32, // 1 param
        Type::I32,
        vec![Type::F32, Type::F64], // 2 locals
        body,
    );
    module.add_function(func);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(
        parsed.functions[FuncId::from_raw_parts(0, 0)].vars().len(),
        2
    );
}

#[test]
fn test_roundtrip_all_sections_together() {
    use crate::ir::module::{Global, GlobalKind, Import};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    // Test module with all major sections
    let mut module = Module::new();

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
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let local_func_id = module.add_function(Function::new(
        "local_func".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    // Globals
    let init = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(100))
    };
    module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Local { init },
    });

    // Table
    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 10,
            maximum: Some(100),
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    // Exports (export the local function, not the imported one)
    module.add_export(
        "exported_func".to_string(),
        crate::ir::module::ExportKind::Function(local_func_id),
    );

    let mut writer = BinaryWriter::new();
    let bytes = writer
        .write_module(&module)
        .expect("Failed to write complete module");

    let mut reader = BinaryReader::new();
    let parsed = reader
        .parse_module(&bytes)
        .expect("Failed to parse complete module");

    // Types may include inferred types from functions
    assert!(parsed.types.len() >= 2, "Expected at least 2 types");
    assert_eq!(parsed.functions.len(), 2);
    assert_eq!(parsed.globals.len(), 1);
    assert!(!parsed.tables.is_empty());
    assert_eq!(parsed.exports.len(), 1);
}

// ========== Import Section Tests ==========

#[test]
fn test_import_function() {
    use crate::ir::module::{Import, ImportKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_import(Import {
        module: "env".to_string(),
        name: "log".to_string(),
        kind: ImportKind::Function(Type::I32, Type::NONE),
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    let func = &parsed.functions[parsed.functions.ids().next().unwrap()];
    let import = if let crate::ir::module::FunctionKind::Import(import) = &func.kind {
        import
    } else {
        panic!("Expected function import");
    };
    assert_eq!(import.module, "env");
    assert_eq!(import.name, "log");
}

#[test]
fn test_import_memory() {
    use crate::ir::module::{Import, ImportKind, MemoryLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

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

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.memories.len(), 1);
}

#[test]
fn test_import_global() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_global(Global {
        name: "global_val".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Import {
            module: "env".to_string(),
            field: "global_val".to_string(),
        },
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.globals.len(), 1);
}

#[test]
fn test_multiple_imports_mixed() {
    use crate::ir::module::{Global, GlobalKind, Import, ImportKind, MemoryLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

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
    module.add_global(Global {
        name: "g".to_string(),
        type_: Type::F32,
        mutable: true,
        kind: GlobalKind::Import {
            module: "js".to_string(),
            field: "g".to_string(),
        },
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

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
    assert_eq!(parsed.globals.len(), 1);
    assert_eq!(parsed.memories.len(), 1);
}

#[test]
fn test_import_many_functions() {
    use crate::ir::module::{Import, ImportKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    for i in 0..50 {
        module.add_import(Import {
            module: "env".to_string(),
            name: format!("func_{}", i),
            kind: ImportKind::Function(Type::I32, Type::I64),
        });
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 50);
}

// ========== Export Section Tests ==========

#[test]
fn test_export_function() {
    use crate::ir::module::ExportKind;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Add a function
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    let func_id = module.add_function(Function::new(
        "my_func".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    // Export it
    module.add_export("exported".to_string(), ExportKind::Function(func_id));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.exports.len(), 1);
    assert_eq!(parsed.exports[0].name, "exported");
}

#[test]
fn test_export_multiple_functions() {
    use crate::ir::module::ExportKind;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    for i in 0..10 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        let func_id = module.add_function(Function::new(
            format!("func_{}", i),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        ));
        module.add_export(format!("export_{}", i), ExportKind::Function(func_id));
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.exports.len(), 10);
    assert_eq!(parsed.functions.len(), 10);
}

#[test]
fn test_export_memory() {
    use crate::ir::module::ExportKind;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(1, Some(100));
    module.add_export(
        "memory".to_string(),
        ExportKind::Memory(MemoryId::from_raw_parts(0, 0)),
    );

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.exports.len(), 1);
    assert!(!parsed.memories.is_empty());
}

#[test]
fn test_export_global() {
    use crate::ir::module::{ExportKind, Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let init = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    let g0_id = module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Local { init },
    });

    module.add_export("my_global".to_string(), ExportKind::Global(g0_id));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.exports.len(), 1);
    assert_eq!(parsed.globals.len(), 1);
}

#[test]
fn test_export_table() {
    use crate::ir::module::{ExportKind, TableLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 10,
            maximum: Some(100),
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    module.add_export(
        "my_table".to_string(),
        ExportKind::Table(TableId::from_raw_parts(0, 0)),
    );

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.exports.len(), 1);
    assert!(!parsed.tables.is_empty());
}

// ========== Memory Section Tests ==========

#[test]
fn test_memory_basic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(1, Some(10));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.memories.is_empty());
    let mem = parsed.memories.unwrap_first();
    assert_eq!(mem.limits.initial, 1);
    assert_eq!(mem.limits.maximum, Some(10));
}

#[test]
fn test_memory_no_maximum() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(5, None);

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.memories.is_empty());
    let mem = parsed.memories.unwrap_first();
    assert_eq!(mem.limits.initial, 5);
    assert_eq!(mem.limits.maximum, None);
}

#[test]
fn test_memory_large_limits() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(100, Some(1000));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.memories.is_empty());
    let mem = parsed.memories.unwrap_first();
    assert_eq!(mem.limits.initial, 100);
    assert_eq!(mem.limits.maximum, Some(1000));
}

// ========== Global Section Tests ==========

#[test]
fn test_global_immutable_i32() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let init = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(123))
    };
    module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Local { init },
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.globals.len(), 1);
    let g0_id = parsed.globals.ids().next().unwrap();
    assert_eq!(parsed.globals[g0_id].type_, Type::I32);
    assert!(!parsed.globals[g0_id].mutable);
}

#[test]
fn test_global_mutable_i64() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let init = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I64(999))
    };
    module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I64,
        mutable: true,
        kind: GlobalKind::Local { init },
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.globals.len(), 1);
    let g0_id = parsed.globals.ids().next().unwrap();
    assert_eq!(parsed.globals[g0_id].type_, Type::I64);
    assert!(parsed.globals[g0_id].mutable);
}

#[test]
fn test_global_all_types() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // i32 global
    let init1 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(1))
    };
    module.add_global(Global {
        name: "g1".to_string(),
        type_: Type::I32,
        mutable: false,
        kind: GlobalKind::Local { init: init1 },
    });

    // i64 global
    let init2 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I64(2))
    };
    module.add_global(Global {
        name: "g2".to_string(),
        type_: Type::I64,
        mutable: true,
        kind: GlobalKind::Local { init: init2 },
    });

    // f32 global
    let init3 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::F32(std::f32::consts::PI))
    };
    module.add_global(Global {
        name: "g3".to_string(),
        type_: Type::F32,
        mutable: false,
        kind: GlobalKind::Local { init: init3 },
    });

    // f64 global
    let init4 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::F64(std::f64::consts::E))
    };
    module.add_global(Global {
        name: "g4".to_string(),
        type_: Type::F64,
        mutable: true,
        kind: GlobalKind::Local { init: init4 },
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.globals.len(), 4);
}

#[test]
fn test_global_many() {
    use crate::ir::module::{Global, GlobalKind};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    for i in 0..100 {
        let init = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(i))
        };
        module.add_global(Global {
            name: format!("g{}", i),
            type_: Type::I32,
            mutable: i % 2 == 0,
            kind: GlobalKind::Local { init },
        });
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.globals.len(), 100);
}

// ========== Element Section Tests ==========

#[test]
fn test_element_basic() {
    use crate::ir::module::{ElementSegment, TableLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Need a table
    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 10,
            maximum: None,
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    // Need functions to reference
    let body0 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    module.add_function(Function::new(
        "f0".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body0,
    ));
    let body1 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    module.add_function(Function::new(
        "f1".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body1,
    ));

    // Element segment
    let offset = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    module.elements.push(ElementSegment {
        table_index: TableId::from_raw_parts(0, 0),
        offset,
        func_indices: vec![FuncId::from_raw_parts(0, 0), FuncId::from_raw_parts(1, 0)],
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.elements.len(), 1);
    assert_eq!(parsed.elements[0].func_indices.len(), 2);
}

#[test]
fn test_element_multiple_segments() {
    use crate::ir::module::{ElementSegment, TableLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 20,
            maximum: None,
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    for _ in 0..5 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        module.add_function(Function::new(
            "f".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        ));
    }

    // Multiple segments at different offsets
    for i in 0..3 {
        let offset = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(i * 5))
        };
        module.elements.push(ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![FuncId::from_raw_parts(0, 0), FuncId::from_raw_parts(1, 0)],
        });
    }

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.elements.len(), 3);
}

#[test]
fn test_element_many_functions() {
    use crate::ir::module::{ElementSegment, TableLimits};
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 100,
            maximum: None,
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    for _ in 0..50 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        module.add_function(Function::new(
            "f".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        ));
    }

    let offset = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    let indices: Vec<u32> = (0..50).collect();
    module.elements.push(ElementSegment {
        table_index: TableId::from_raw_parts(0, 0),
        offset,
        func_indices: indices
            .into_iter()
            .map(|i| FuncId::from_raw_parts(i, 0))
            .collect(),
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.elements.len(), 1);
    assert_eq!(parsed.elements[0].func_indices.len(), 50);
}

// ========== Data Section Tests ==========

#[test]
fn test_data_basic() {
    use crate::ir::module::DataSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(1, None);

    let offset = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset,
        data: vec![1, 2, 3, 4, 5],
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.data.len(), 1);
    assert_eq!(parsed.data[0].data, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_data_multiple_segments() {
    use crate::ir::module::DataSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(1, None);

    // Segment 1 at offset 0
    let offset1 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset: offset1,
        data: vec![1, 2, 3],
    });

    // Segment 2 at offset 100
    let offset2 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(100))
    };
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset: offset2,
        data: vec![4, 5, 6],
    });

    // Segment 3 at offset 200
    let offset3 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(200))
    };
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset: offset3,
        data: vec![7, 8, 9],
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.data.len(), 3);
}

#[test]
fn test_data_large() {
    use crate::ir::module::DataSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(10, None);

    let offset = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset,
        data: large_data.clone(),
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.data.len(), 1);
    assert_eq!(parsed.data[0].data.len(), 10000);
    assert_eq!(parsed.data[0].data, large_data);
}

#[test]
fn test_data_empty() {
    use crate::ir::module::DataSegment;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.set_memory(1, None);

    let offset = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(0))
    };
    module.data.push(DataSegment {
        memory_index: MemoryId::from_raw_parts(0, 0),
        offset,
        data: vec![],
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.data.len(), 1);
    assert_eq!(parsed.data[0].data.len(), 0);
}

// ========== Start Section Tests ==========

#[test]
fn test_start_function() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    module.add_function(Function::new(
        "start".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));
    module.start = Some(FuncId::from_raw_parts(0, 0));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.start, Some(FuncId::from_raw_parts(0, 0)));
}

#[test]
fn test_start_with_multiple_functions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let mut start_id = None;
    for i in 0..10 {
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        let id = module.add_function(Function::new(
            format!("func_{}", i),
            Type::NONE,
            Type::NONE,
            vec![],
            body,
        ));
        if i == 5 {
            start_id = Some(id);
        }
    }

    module.start = start_id; // Start at function 5

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(parsed.start.is_some());
    let parsed_start = parsed.start.unwrap();
    assert_eq!(parsed.functions[parsed_start].name, "func_5");
    assert_eq!(parsed.functions.len(), 10);
}

#[test]
fn test_no_start_function() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.nop()
    };
    module.add_function(Function::new(
        "f".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));
    // No start set

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.start, None);
}

// ========== Table Section Tests ==========

#[test]
fn test_table_basic() {
    use crate::ir::module::TableLimits;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 5,
            maximum: Some(50),
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.tables.is_empty());
    let table = parsed.tables.unwrap_first();
    assert_eq!(table.limits.initial, 5);
    assert_eq!(table.limits.maximum, Some(50));
}

#[test]
fn test_table_no_maximum() {
    use crate::ir::module::TableLimits;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 10,
            maximum: None,
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.tables.is_empty());
    let table = parsed.tables.unwrap_first();
    assert_eq!(table.limits.initial, 10);
    assert_eq!(table.limits.maximum, None);
}

#[test]
fn test_table_large_limits() {
    use crate::ir::module::TableLimits;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    module.add_table(Table {
        name: "".to_string(),
        limits: TableLimits {
            initial: 1000,
            maximum: Some(10000),
            element_type: Type::FUNCREF,
        },
        kind: TableKind::Local,
    });

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert!(!parsed.tables.is_empty());
    let table = parsed.tables.unwrap_first();
    assert_eq!(table.limits.initial, 1000);
    assert_eq!(table.limits.maximum, Some(10000));
}

// ========== Phase 1: Control Flow Instructions ==========

#[test]
fn test_unreachable_instruction() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.unreachable()
    };
    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert!(parsed.functions[FuncId::from_raw_parts(0, 0)]
        .body()
        .is_some());
}

#[test]
fn test_return_void() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.return_(None)
    };
    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_return_with_value() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I32(42));
        builder.return_(Some(value))
    };
    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_br_unconditional() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // block with unconditional break
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let label_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let br = builder.break_(label_block, None, None, Type::NONE);
        let mut block_list = vec![];
        block_list.push(br);

        builder.arena[label_block].kind = ExpressionKind::Block {
            name: Some("label".to_string()),
            list: block_list,
        };
        label_block
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_br_if_conditional() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // block with conditional break
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let label_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let condition = builder.const_(Literal::I32(1));
        let br_if = builder.break_(label_block, Some(condition), None, Type::NONE);
        let mut block_list = vec![];
        block_list.push(br_if);
        builder.arena[label_block].kind = ExpressionKind::Block {
            name: Some("label".to_string()),
            list: block_list,
        };
        label_block
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_br_with_value() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // block that returns a value via break
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let label_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::I32,
        });

        let value = builder.const_(Literal::I32(42));
        let br = builder.break_(label_block, None, Some(value), Type::I32);
        let mut block_list = vec![];
        block_list.push(br);
        builder.arena[label_block].kind = ExpressionKind::Block {
            name: Some("label".to_string()),
            list: block_list,
        };
        label_block
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_nested_blocks_with_breaks() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // outer block
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let inner_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });
        let outer_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let br_inner = builder.break_(inner_block, None, None, Type::NONE);
        let mut inner_list = vec![];
        inner_list.push(br_inner);
        builder.arena[inner_block].kind = ExpressionKind::Block {
            name: Some("inner".to_string()),
            list: inner_list,
        };

        let br_outer = builder.break_(outer_block, None, None, Type::NONE);
        let mut outer_list = vec![];
        outer_list.push(inner_block);
        outer_list.push(br_outer);
        builder.arena[outer_block].kind = ExpressionKind::Block {
            name: Some("outer".to_string()),
            list: outer_list,
        };
        outer_block
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_loop_basic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let loop_body = builder.nop();
        builder.loop_(Some("loop"), loop_body, Type::NONE)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_loop_with_break_to_continue() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // break to loop label continues the loop
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let loop_expr = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let br = builder.break_(loop_expr, None, None, Type::NONE);

        builder.arena[loop_expr].kind = ExpressionKind::Loop {
            name: Some("loop".to_string()),
            body: br,
        };
        builder.arena[loop_expr].type_ = Type::NONE;
        loop_expr
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_loop_with_exit_block() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // break to outer block exits the loop
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let exit_block = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let br = builder.break_(exit_block, None, None, Type::NONE);
        let loop_body = builder.loop_(Some("loop"), br, Type::NONE);
        let mut block_list = vec![];
        block_list.push(loop_body);

        builder.arena[exit_block].kind = ExpressionKind::Block {
            name: Some("exit".to_string()),
            list: block_list,
        };
        builder.arena[exit_block].type_ = Type::NONE;
        exit_block
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_call_function() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Function to be called
    let callee_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    module.add_function(Function::new(
        "callee".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        callee_body,
    ));

    // Caller function
    let caller_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let operands = vec![];
        builder.call("callee", operands, Type::I32, false)
    };
    module.add_function(Function::new(
        "caller".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        caller_body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
}

#[test]
fn test_call_with_arguments() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Callee: takes two i32, returns i32
    let callee_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let param1 = builder.local_get(0, Type::I32);
        let param2 = builder.local_get(1, Type::I32);
        builder.binary(BinaryOp::AddInt32, param1, param2, Type::I32)
    };
    module.add_function(Function::new(
        "add".to_string(),
        Type::NONE, // params handled via add_type
        Type::I32,
        vec![Type::I32, Type::I32],
        callee_body,
    ));

    // Caller: passes two constants
    let caller_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let mut operands = vec![];
        operands.push(builder.const_(Literal::I32(10)));
        operands.push(builder.const_(Literal::I32(20)));
        builder.call("add", operands, Type::I32, false)
    };
    module.add_function(Function::new(
        "caller".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        caller_body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
}

#[test]
fn test_tail_call() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Callee
    let callee_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(42))
    };
    module.add_function(Function::new(
        "callee".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        callee_body,
    ));

    // Caller with tail call (is_return=true)
    let caller_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let operands = vec![];
        builder.call("callee", operands, Type::I32, true)
    };
    module.add_function(Function::new(
        "caller".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        caller_body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 2);
}

#[test]
fn test_complex_control_flow() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // if (param0) { return 1; } else { return 0; }
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let condition = builder.local_get(0, Type::I32);
        let true_val = builder.const_(Literal::I32(1));
        let false_val = builder.const_(Literal::I32(0));
        builder.if_("", condition, true_val, Some(false_val), Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_deeply_nested_control_flow() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Nest 10 levels deep: block -> loop -> if -> block -> ...
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let mut expr = builder.const_(Literal::I32(42));
        for i in 0..10 {
            let name = format!("level{}", i);
            if i % 3 == 0 {
                let mut list = vec![];
                list.push(expr);
                expr = builder.block(Some(name.as_str()), list, Type::I32);
            } else if i % 3 == 1 {
                expr = builder.loop_(Some(name.as_str()), expr, Type::I32);
            } else {
                let condition = builder.const_(Literal::I32(1));
                expr = builder.if_(None::<String>, condition, expr, None, Type::I32);
            }
        }
        expr
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_select_instruction() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let condition = builder.const_(Literal::I32(1));
        let if_true = builder.const_(Literal::I32(10));
        let if_false = builder.const_(Literal::I32(20));
        builder.select(condition, if_true, if_false, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_drop_instruction() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I32(42));
        builder.drop(value)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_local_tee_instruction() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // local.tee sets a local and leaves value on stack
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I32(42));
        builder.local_tee(0, value, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32], // One local
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ========== Phase 2: Parametric & Variable Instructions ==========

#[test]
fn test_multiple_local_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // local[0] = 10; local[1] = 20; return local[0] + local[1]
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let c1 = builder.const_(Literal::I32(10));
        let set0 = builder.local_set(LocalId::from(0), c1);
        let c2 = builder.const_(Literal::I32(20));
        let set1 = builder.local_set(LocalId::from(1), c2);
        let get0 = builder.local_get(LocalId::from(0), Type::I32);
        let get1 = builder.local_get(LocalId::from(1), Type::I32);
        let add = builder.binary(BinaryOp::AddInt32, get0, get1, Type::I32);

        let mut list = vec![];
        list.push(set0);
        list.push(set1);
        list.push(add);
        builder.block(None::<&str>, list, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32, Type::I32],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_local_tee_chaining() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // local.tee 0 (local.tee 1 (const 42)) - sets both locals to 42
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let const_val = builder.const_(Literal::I32(42));
        let tee1 = builder.local_tee(1, const_val, Type::I32);
        builder.local_tee(0, tee1, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32, Type::I32],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_local_swap_pattern() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Swap locals: temp = local[0]; local[0] = local[1]; local[1] = temp
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let get0 = builder.local_get(0, Type::I32);
        let tee_temp = builder.local_tee(2, get0, Type::I32);
        let drop_temp = builder.drop(tee_temp);
        let get1 = builder.local_get(1, Type::I32);
        let set0 = builder.local_set(0, get1);
        let get_temp = builder.local_get(2, Type::I32);
        let set1 = builder.local_set(1, get_temp);
        let result = builder.local_get(0, Type::I32);

        let mut list = vec![];
        list.push(drop_temp);
        list.push(set0);
        list.push(set1);
        list.push(result);
        builder.block("", list, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![Type::I32, Type::I32, Type::I32], // params + temp
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_select_with_f32() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let condition = builder.const_(Literal::I32(0));
        let if_true = builder.const_(Literal::F32(1.5));
        let if_false = builder.const_(Literal::F32(2.5));
        builder.select(condition, if_true, if_false, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_select_with_i64() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let condition = builder.const_(Literal::I32(1));
        let if_true = builder.const_(Literal::I64(100));
        let if_false = builder.const_(Literal::I64(200));
        builder.select(condition, if_true, if_false, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_select_nested() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // select(c1, select(c2, a, b), c)
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let c2 = builder.const_(Literal::I32(1));
        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let inner = builder.select(c2, a, b, Type::I32);

        let c1 = builder.const_(Literal::I32(0));
        let c = builder.const_(Literal::I32(30));
        builder.select(c1, inner, c, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_drop_multiple_values() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Drop multiple computed values
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let v1 = builder.const_(Literal::I32(10));
        let v2 = builder.const_(Literal::I32(20));
        let sum = builder.binary(BinaryOp::AddInt32, v1, v2, Type::I32);
        let drop1 = builder.drop(sum);

        let v3 = builder.const_(Literal::I64(100));
        let drop2 = builder.drop(v3);

        let result = builder.const_(Literal::I32(42));

        let mut list = vec![];
        list.push(drop1);
        list.push(drop2);
        list.push(result);
        builder.block("", list, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_global_operations_complex() {
    use crate::ir::module::Global;
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Add two globals
    let init1 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(100))
    };
    let g0_id = module.add_global(Global {
        name: "g0".to_string(),
        type_: Type::I32,
        mutable: true,
        kind: GlobalKind::Local { init: init1 },
    });

    let init2 = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        builder.const_(Literal::I32(200))
    };
    let g1_id = module.add_global(Global {
        name: "g1".to_string(),
        type_: Type::I32,
        mutable: true,
        kind: GlobalKind::Local { init: init2 },
    });

    // Function: swap globals and return their sum
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let get0 = builder.global_get(g0_id, Type::I32);
        let get1 = builder.global_get(g1_id, Type::I32);
        let set0 = builder.global_set(g0_id, get1);
        let set1 = builder.global_set(g1_id, get0);
        let get0_new = builder.global_get(g0_id, Type::I32);
        let get1_new = builder.global_get(g1_id, Type::I32);
        let result = builder.binary(BinaryOp::AddInt32, get0_new, get1_new, Type::I32);

        let mut list = vec![];
        list.push(set0);
        list.push(set1);
        list.push(result);
        builder.block("", list, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
    assert_eq!(parsed.globals.len(), 2);
}

#[test]
fn test_many_locals_access() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Sum 10 locals
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let mut list = vec![];

        // Initialize locals
        for i in 0..10 {
            let val = builder.const_(Literal::I32(i as i32));
            let set = builder.local_set(i, val);
            list.push(set);
        }

        // Sum them all
        let mut sum = builder.local_get(0, Type::I32);
        for i in 1..10 {
            let get = builder.local_get(i, Type::I32);
            sum = builder.binary(BinaryOp::AddInt32, sum, get, Type::I32);
        }
        list.push(sum);

        builder.block("", list, Type::I32)
    };

    let locals = vec![Type::I32; 10];
    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        locals,
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ========== Phase 3: Integer Numeric Operations ==========

// i32 arithmetic operations
#[test]
fn test_i32_arithmetic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Test arithmetic chain: (10 + 20) * 3 / 2 - 1
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::I32(10));
        let b = builder.const_(Literal::I32(20));
        let add = builder.binary(BinaryOp::AddInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(3));
        let mul = builder.binary(BinaryOp::MulInt32, add, c, Type::I32);

        let d = builder.const_(Literal::I32(2));
        let div = builder.binary(BinaryOp::DivSInt32, mul, d, Type::I32);

        let e = builder.const_(Literal::I32(1));
        builder.binary(BinaryOp::SubInt32, div, e, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_division_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::I32(100));
        let b = builder.const_(Literal::I32(3));
        let div_s = builder.binary(BinaryOp::DivSInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(100));
        let d = builder.const_(Literal::I32(3));
        let div_u = builder.binary(BinaryOp::DivUInt32, c, d, Type::I32);

        builder.binary(BinaryOp::AddInt32, div_s, div_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_remainder() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::I32(17));
        let b = builder.const_(Literal::I32(5));
        let rem_s = builder.binary(BinaryOp::RemSInt32, a, b, Type::I32);

        let c = builder.const_(Literal::I32(17));
        let d = builder.const_(Literal::I32(5));
        let rem_u = builder.binary(BinaryOp::RemUInt32, c, d, Type::I32);

        builder.binary(BinaryOp::MulInt32, rem_s, rem_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_bitwise() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::XorInt32, r1, xor, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_shifts() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::XorInt32, r1, shr_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_rotates() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I32(0xABCD1234u32 as i32));
        let amt = builder.const_(Literal::I32(8));
        let rotl = builder.binary(BinaryOp::RotLInt32, value, amt, Type::I32);

        let value2 = builder.const_(Literal::I32(0xABCD1234u32 as i32));
        let amt2 = builder.const_(Literal::I32(8));
        let rotr = builder.binary(BinaryOp::RotRInt32, value2, amt2, Type::I32);

        builder.binary(BinaryOp::XorInt32, rotl, rotr, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_comparisons() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::OrInt32, r1, r2, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_comparison_variants() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt32, r1, r2, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_unary_all() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt32, r2, eqz, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// i64 operations
#[test]
fn test_i64_arithmetic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::I64(1000));
        let b = builder.const_(Literal::I64(300));
        let add = builder.binary(BinaryOp::AddInt64, a, b, Type::I64);

        let c = builder.const_(Literal::I64(2000));
        let d = builder.const_(Literal::I64(500));
        let sub = builder.binary(BinaryOp::SubInt64, c, d, Type::I64);

        let mul = builder.binary(BinaryOp::MulInt64, add, sub, Type::I64);

        let e = builder.const_(Literal::I64(10));
        builder.binary(BinaryOp::DivSInt64, mul, e, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_bitwise() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::XorInt64, r1, xor, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_shifts() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::XorInt64, r1, shr_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_rotates() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I64(0xFEDCBA9876543210u64 as i64));
        let amt = builder.const_(Literal::I64(16));
        let rotl = builder.binary(BinaryOp::RotLInt64, value, amt, Type::I64);

        let value2 = builder.const_(Literal::I64(0xFEDCBA9876543210u64 as i64));
        let amt2 = builder.const_(Literal::I64(16));
        let rotr = builder.binary(BinaryOp::RotRInt64, value2, amt2, Type::I64);

        builder.binary(BinaryOp::XorInt64, rotl, rotr, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_comparisons() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt32, r1, r2, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_unary_all() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt64, r2, eqz, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_mixed_i32_i64_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Use i32 for comparison, i64 for computation
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let i32_val = builder.const_(Literal::I32(100));
        let i64_a = builder.const_(Literal::I64(1000));
        let i64_b = builder.const_(Literal::I64(2000));

        let i64_sum = builder.binary(BinaryOp::AddInt64, i64_a, i64_b, Type::I64);
        let comparison = builder.unary(UnaryOp::EqZInt32, i32_val, Type::I32);

        // Select between sum and constant based on comparison
        let fallback = builder.const_(Literal::I64(9999));
        builder.select(comparison, i64_sum, fallback, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_extreme_integer_values() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    // Test with extreme values
    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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

        let c0 = builder.const_(Literal::I64(0));
        builder.select(comparison, r4, c0, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ============================================================================
// Phase 4: Float Numeric Operations
// ============================================================================

#[test]
fn test_f32_arithmetic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::F32(10.5));
        let b = builder.const_(Literal::F32(20.3));
        let add = builder.binary(BinaryOp::AddFloat32, a, b, Type::F32);

        let c = builder.const_(Literal::F32(3.0));
        let mul = builder.binary(BinaryOp::MulFloat32, add, c, Type::F32);

        let d = builder.const_(Literal::F32(2.0));
        let div = builder.binary(BinaryOp::DivFloat32, mul, d, Type::F32);

        let e = builder.const_(Literal::F32(1.5));
        builder.binary(BinaryOp::SubFloat32, div, e, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_comparisons() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt32, r1, gt, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_math_functions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddFloat32, r1, r2, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_advanced_math() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let val1 = builder.const_(Literal::F32(4.8));
        let trunc_val = builder.unary(UnaryOp::TruncFloat32, val1, Type::F32);

        let val2 = builder.const_(Literal::F32(4.5));
        let nearest = builder.unary(UnaryOp::NearestFloat32, val2, Type::F32);

        let val3 = builder.const_(Literal::F32(16.0));
        let sqrt_val = builder.unary(UnaryOp::SqrtFloat32, val3, Type::F32);

        let r1 = builder.binary(BinaryOp::AddFloat32, trunc_val, nearest, Type::F32);
        builder.binary(BinaryOp::AddFloat32, r1, sqrt_val, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_min_max() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::F32(10.5));
        let b = builder.const_(Literal::F32(20.3));
        let min_val = builder.binary(BinaryOp::MinFloat32, a, b, Type::F32);

        let c = builder.const_(Literal::F32(5.0));
        let d = builder.const_(Literal::F32(15.0));
        let max_val = builder.binary(BinaryOp::MaxFloat32, c, d, Type::F32);

        builder.binary(BinaryOp::AddFloat32, min_val, max_val, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_copysign() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::F32(42.0));
        let b = builder.const_(Literal::F32(-1.0));
        builder.binary(BinaryOp::CopySignFloat32, a, b, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_arithmetic() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::F64(100.5));
        let b = builder.const_(Literal::F64(200.3));
        let add = builder.binary(BinaryOp::AddFloat64, a, b, Type::F64);

        let c = builder.const_(Literal::F64(3.0));
        let mul = builder.binary(BinaryOp::MulFloat64, add, c, Type::F64);

        let d = builder.const_(Literal::F64(2.0));
        let div = builder.binary(BinaryOp::DivFloat64, mul, d, Type::F64);

        let e = builder.const_(Literal::F64(1.5));
        builder.binary(BinaryOp::SubFloat64, div, e, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_comparisons() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddInt32, r1, ge, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_math_functions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let val1 = builder.const_(Literal::F64(-142.7));
        let abs_val = builder.unary(UnaryOp::AbsFloat64, val1, Type::F64);

        let val2 = builder.const_(Literal::F64(std::f64::consts::PI));
        let neg_val = builder.unary(UnaryOp::NegFloat64, val2, Type::F64);

        let val3 = builder.const_(Literal::F64(64.0));
        let sqrt_val = builder.unary(UnaryOp::SqrtFloat64, val3, Type::F64);

        let r1 = builder.binary(BinaryOp::AddFloat64, abs_val, neg_val, Type::F64);
        builder.binary(BinaryOp::AddFloat64, r1, sqrt_val, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_rounding() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.binary(BinaryOp::AddFloat64, r1, r2, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_min_max() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let a = builder.const_(Literal::F64(100.5));
        let b = builder.const_(Literal::F64(200.3));
        let min_val = builder.binary(BinaryOp::MinFloat64, a, b, Type::F64);

        let c = builder.const_(Literal::F64(50.0));
        let d = builder.const_(Literal::F64(150.0));
        let max_val = builder.binary(BinaryOp::MaxFloat64, c, d, Type::F64);

        builder.binary(BinaryOp::AddFloat64, min_val, max_val, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_mixed_f32_f64_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
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
        builder.select(cmp, f64_result, alt, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ============================================================================
// Phase 5: Conversion & Reinterpret Operations
// ============================================================================

#[test]
fn test_i32_to_f32_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let signed = builder.const_(Literal::I32(-42));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt32ToFloat32, signed, Type::F32);

        let unsigned = builder.const_(Literal::I32(100));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt32ToFloat32, unsigned, Type::F32);

        builder.binary(BinaryOp::AddFloat32, s_to_f, u_to_f, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_to_f32_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let signed = builder.const_(Literal::I64(-1000));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt64ToFloat32, signed, Type::F32);

        let unsigned = builder.const_(Literal::I64(2000));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt64ToFloat32, unsigned, Type::F32);

        builder.binary(BinaryOp::AddFloat32, s_to_f, u_to_f, Type::F32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_to_f64_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let signed = builder.const_(Literal::I32(-42));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt32ToFloat64, signed, Type::F64);

        let unsigned = builder.const_(Literal::I32(100));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt32ToFloat64, unsigned, Type::F64);

        builder.binary(BinaryOp::AddFloat64, s_to_f, u_to_f, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_to_f64_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let signed = builder.const_(Literal::I64(-1000));
        let s_to_f = builder.unary(UnaryOp::ConvertSInt64ToFloat64, signed, Type::F64);

        let unsigned = builder.const_(Literal::I64(2000));
        let u_to_f = builder.unary(UnaryOp::ConvertUInt64ToFloat64, unsigned, Type::F64);

        builder.binary(BinaryOp::AddFloat64, s_to_f, u_to_f, Type::F64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_to_i32_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let float_val = builder.const_(Literal::F32(42.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat32ToInt32, float_val, Type::I32);

        let float_val2 = builder.const_(Literal::F32(100.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat32ToInt32, float_val2, Type::I32);

        builder.binary(BinaryOp::AddInt32, trunc_s, trunc_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_to_i32_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let float_val = builder.const_(Literal::F64(42.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat64ToInt32, float_val, Type::I32);

        let float_val2 = builder.const_(Literal::F64(100.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat64ToInt32, float_val2, Type::I32);

        builder.binary(BinaryOp::AddInt32, trunc_s, trunc_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_to_i64_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let float_val = builder.const_(Literal::F32(142.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat32ToInt64, float_val, Type::I64);

        let float_val2 = builder.const_(Literal::F32(200.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat32ToInt64, float_val2, Type::I64);

        builder.binary(BinaryOp::AddInt64, trunc_s, trunc_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_to_i64_conversions() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let float_val = builder.const_(Literal::F64(142.7));
        let trunc_s = builder.unary(UnaryOp::TruncSFloat64ToInt64, float_val, Type::I64);

        let float_val2 = builder.const_(Literal::F64(200.3));
        let trunc_u = builder.unary(UnaryOp::TruncUFloat64ToInt64, float_val2, Type::I64);

        builder.binary(BinaryOp::AddInt64, trunc_s, trunc_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ============================================================================
// Phase 6: Memory Operations
// ============================================================================

#[test]
fn test_i32_load_store() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Load from address 0, add 10, store back
        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(0, 4, false, 0, 2, addr, Type::I32);

        let ten = builder.const_(Literal::I32(10));
        let added = builder.binary(BinaryOp::AddInt32, loaded, ten, Type::I32);

        let store_addr = builder.const_(Literal::I32(0));
        builder.store(MemoryId::from_raw_parts(0, 0), 4, 4, 0, store_addr, added)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_load_store() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr = builder.const_(Literal::I32(8));
        let loaded = builder.load(0, 8, false, 0, 3, addr, Type::I64);

        let multiplier = builder.const_(Literal::I64(2));
        let result = builder.binary(BinaryOp::MulInt64, loaded, multiplier, Type::I64);

        let store_addr = builder.const_(Literal::I32(16));
        builder.store(MemoryId::from_raw_parts(0, 0), 8, 8, 0, store_addr, result)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_load8_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(0, 1, true, 0, 0, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(1));
        let load_u = builder.load(0, 1, false, 0, 0, addr2, Type::I32);

        builder.binary(BinaryOp::AddInt32, load_s, load_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_load16_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(0, 2, true, 0, 1, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(2));
        let load_u = builder.load(0, 2, false, 0, 1, addr2, Type::I32);

        builder.binary(BinaryOp::SubInt32, load_s, load_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_load8_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(0, 1, true, 0, 0, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(1));
        let load_u = builder.load(0, 1, false, 0, 0, addr2, Type::I64);

        builder.binary(BinaryOp::AddInt64, load_s, load_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_load16_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(0, 2, true, 0, 1, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(2));
        let load_u = builder.load(0, 2, false, 0, 1, addr2, Type::I64);

        builder.binary(BinaryOp::SubInt64, load_s, load_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_load32_signed_unsigned() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr1 = builder.const_(Literal::I32(0));
        let load_s = builder.load(0, 4, true, 0, 2, addr1, Type::I64);

        let addr2 = builder.const_(Literal::I32(4));
        let load_u = builder.load(0, 4, false, 0, 2, addr2, Type::I64);

        builder.binary(BinaryOp::MulInt64, load_s, load_u, Type::I64)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f32_load_store() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(0, 4, false, 0, 2, addr, Type::F32);

        let multiplier = builder.const_(Literal::F32(2.0));
        let result = builder.binary(BinaryOp::MulFloat32, loaded, multiplier, Type::F32);

        let store_addr = builder.const_(Literal::I32(4));
        builder.store(MemoryId::from_raw_parts(0, 0), 4, 4, 0, store_addr, result)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_f64_load_store() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(0, 8, false, 0, 3, addr, Type::F64);

        let divisor = builder.const_(Literal::F64(2.0));
        let result = builder.binary(BinaryOp::DivFloat64, loaded, divisor, Type::F64);

        let store_addr = builder.const_(Literal::I32(8));
        builder.store(MemoryId::from_raw_parts(0, 0), 8, 8, 0, store_addr, result)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i32_store8_store16() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value8 = builder.const_(Literal::I32(255));
        let addr8 = builder.const_(Literal::I32(0));
        let store8 = builder.store(MemoryId::from_raw_parts(0, 0), 1, 1, 0, addr8, value8);

        let value16 = builder.const_(Literal::I32(65535));
        let addr16 = builder.const_(Literal::I32(4));
        let store16 = builder.store(MemoryId::from_raw_parts(0, 0), 2, 2, 0, addr16, value16);

        builder.block("", vec![store8, store16], Type::NONE)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_i64_store8_store16_store32() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value8 = builder.const_(Literal::I64(255));
        let addr8 = builder.const_(Literal::I32(0));
        let store8 = builder.store(MemoryId::from_raw_parts(0, 0), 1, 1, 0, addr8, value8);

        let value16 = builder.const_(Literal::I64(65535));
        let addr16 = builder.const_(Literal::I32(8));
        let store16 = builder.store(MemoryId::from_raw_parts(0, 0), 2, 2, 0, addr16, value16);

        let value32 = builder.const_(Literal::I64(0xFFFFFFFF));
        let addr32 = builder.const_(Literal::I32(16));
        let store32 = builder.store(MemoryId::from_raw_parts(0, 0), 4, 4, 0, addr32, value32);

        builder.block("", vec![store8, store16, store32], Type::NONE)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_load_with_offset() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let base = builder.const_(Literal::I32(100));
        let loaded1 = builder.load(0, 4, false, 0, 2, base, Type::I32);

        let base2 = builder.const_(Literal::I32(100));
        let loaded2 = builder.load(0, 4, false, 16, 2, base2, Type::I32);

        builder.binary(BinaryOp::AddInt32, loaded1, loaded2, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_store_with_offset() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let value = builder.const_(Literal::I32(42));
        let base = builder.const_(Literal::I32(1000));
        builder.store(MemoryId::from_raw_parts(0, 0), 4, 4, 100, base, value)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_memory_size() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let size = builder.memory_size(0);
        let multiplier = builder.const_(Literal::I32(65536));
        builder.binary(BinaryOp::MulInt32, size, multiplier, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_memory_grow() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        let pages = builder.const_(Literal::I32(1));
        let old_size = builder.memory_grow(0, pages);

        let new_size = builder.memory_size(0);
        builder.binary(BinaryOp::AddInt32, old_size, new_size, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_complex_memory_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Load i32, i64, f32, f64 from different addresses
        let addr1 = builder.const_(Literal::I32(0));
        let i32_val = builder.load(0, 4, false, 0, 2, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(8));
        let _i64_val = builder.load(0, 8, false, 0, 3, addr2, Type::I64);

        let addr3 = builder.const_(Literal::I32(16));
        let f32_val = builder.load(0, 4, false, 0, 2, addr3, Type::F32);

        // Do some operations
        let i32_converted = builder.unary(UnaryOp::ConvertSInt32ToFloat32, i32_val, Type::F32);
        let sum = builder.binary(BinaryOp::AddFloat32, i32_converted, f32_val, Type::F32);

        // Store result
        let store_addr = builder.const_(Literal::I32(32));
        builder.store(MemoryId::from_raw_parts(0, 0), 4, 4, 0, store_addr, sum)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::NONE,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

// ============================================================================
// Phase 7: Validation & Edge Cases
// ============================================================================

#[test]
fn test_deeply_nested_blocks() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Create 20 levels of nested blocks
        let value = builder.const_(Literal::I32(42));
        let mut current = value;

        for i in 0..20 {
            let label = format!("block_{}", i);
            current = builder.block(Some(label.as_str()), vec![current], Type::I32);
        }
        current
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_deeply_nested_ifs() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Create 15 levels of nested if statements
        let mut current = builder.const_(Literal::I32(1));

        for _ in 0..15 {
            let condition = builder.const_(Literal::I32(1));
            let false_branch = builder.const_(Literal::I32(0));
            current = builder.if_("", condition, current, Some(false_branch), Type::I32);
        }
        current
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_all_operations_combined() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Integer operations
        let i32_a = builder.const_(Literal::I32(10));
        let i32_b = builder.const_(Literal::I32(20));
        let i32_sum = builder.binary(BinaryOp::AddInt32, i32_a, i32_b, Type::I32);
        let c2 = builder.const_(Literal::I32(2));
        let i32_mul = builder.binary(BinaryOp::MulInt32, i32_sum, c2, Type::I32);

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
        let store_op = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            4,
            0,
            store_addr,
            f32_to_i32,
        );

        let load_addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(0, 4, false, 0, 2, load_addr, Type::I32);

        // Control flow
        let c50 = builder.const_(Literal::I32(50));
        let condition = builder.binary(BinaryOp::GtSInt32, loaded, c50, Type::I32);
        let if_true = builder.const_(Literal::I32(100));
        let if_false = builder.const_(Literal::I32(200));
        let selected = builder.if_("", condition, if_true, Some(if_false), Type::I32);

        // Combine everything in a block
        builder.block("", vec![store_op, selected], Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_complex_expression_chain() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);
        // Build a complex expression chain with 50+ operations
        let mut result = builder.const_(Literal::I32(1));

        for i in 1..=25 {
            let next_val = builder.const_(Literal::I32(i));
            result = builder.binary(BinaryOp::AddInt32, result, next_val, Type::I32);

            let multiplier = builder.const_(Literal::I32(2));
            result = builder.binary(BinaryOp::MulInt32, result, multiplier, Type::I32);
        }
        result
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_mixed_type_operations_i32_i64() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

    // Mix i32 and i64 operations with proper type handling
    let i32_val = builder.const_(Literal::I32(42));
    let c2 = builder.const_(Literal::I32(2));
    let i32_doubled = builder.binary(BinaryOp::MulInt32, i32_val, c2, Type::I32);

    let i64_val1 = builder.const_(Literal::I64(1000));
    let i64_val2 = builder.const_(Literal::I64(1000));
    let i64_squared = builder.binary(BinaryOp::MulInt64, i64_val1, i64_val2, Type::I64);

    // Use select to choose between the results (need same type)
    let c50 = builder.const_(Literal::I32(50));
    let i32_compare = builder.binary(BinaryOp::GtSInt32, i32_doubled, c50, Type::I32);
    let c999 = builder.const_(Literal::I64(999));
    let body = builder.select(i32_compare, i64_squared, c999, Type::I64);

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_mixed_type_operations_floats() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

    // Mix f32 and f64 operations
    let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
    let f32_result = builder.unary(UnaryOp::SqrtFloat32, f32_val, Type::F32);

    let f64_val1 = builder.const_(Literal::F64(std::f64::consts::E));
    let f64_val2 = builder.const_(Literal::F64(std::f64::consts::E));
    let f64_result = builder.binary(BinaryOp::MulFloat64, f64_val1, f64_val2, Type::F64);

    // Compare f32 value
    let c1f32 = builder.const_(Literal::F32(1.0));
    let f32_cmp = builder.binary(BinaryOp::GtFloat32, f32_result, c1f32, Type::I32);
    let c1f64 = builder.const_(Literal::F64(1.0));
    let body = builder.select(f32_cmp, f64_result, c1f64, Type::F64);

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::F64,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_all_unary_operations_combined() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

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
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_all_comparison_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    let mut builder = IrBuilder::new(&mut module.expressions);

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
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_memory_operations_sequence() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Complex memory operation sequence
        let addr1 = builder.const_(Literal::I32(0));
        let val1 = builder.load(MemoryId::from_u32(0), 4, false, 0, 2, addr1, Type::I32);

        let addr2 = builder.const_(Literal::I32(4));
        let val2 = builder.load(MemoryId::from_u32(0), 4, false, 0, 2, addr2, Type::I32);

        let sum = builder.binary(BinaryOp::AddInt32, val1, val2, Type::I32);

        let store_addr = builder.const_(Literal::I32(8));
        let store1 = builder.store(MemoryId::from_u32(0), 4, 0, 4, store_addr, sum);

        let load_back_addr = builder.const_(Literal::I32(8));
        let loaded_back = builder.load(
            MemoryId::from_u32(0),
            4,
            false,
            0,
            2,
            load_back_addr,
            Type::I32,
        );

        builder.block(None::<&str>, vec![store1, loaded_back], Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_all_conversion_paths() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Test all conversion paths in one function
        let i32_val = builder.const_(Literal::I32(42));

        // i32 -> f32 -> f64 -> i64 -> i32
        let i32_to_f32 = builder.unary(UnaryOp::ConvertSInt32ToFloat32, i32_val, Type::F32);
        let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_sum = builder.binary(BinaryOp::AddFloat32, i32_to_f32, f32_val, Type::F32);

        let f32_to_i64 = builder.unary(UnaryOp::TruncSFloat32ToInt64, f32_sum, Type::I64);
        let c2_i64 = builder.const_(Literal::I64(2));
        let i64_doubled = builder.binary(BinaryOp::MulInt64, f32_to_i64, c2_i64, Type::I64);

        let i64_to_f64 = builder.unary(UnaryOp::ConvertSInt64ToFloat64, i64_doubled, Type::F64);
        let f64_val = builder.const_(Literal::F64(1.5));
        let f64_div = builder.binary(BinaryOp::DivFloat64, i64_to_f64, f64_val, Type::F64);

        builder.unary(UnaryOp::TruncSFloat64ToInt32, f64_div, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_loop_with_complex_body() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let loop_body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create a loop with complex operations inside
        let i32_val = builder.const_(Literal::I32(10));
        let c2_i32 = builder.const_(Literal::I32(2));
        let i32_mul = builder.binary(BinaryOp::MulInt32, i32_val, c2_i32, Type::I32);

        let f32_val = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_sqrt = builder.unary(UnaryOp::SqrtFloat32, f32_val, Type::F32);
        let f32_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, f32_sqrt, Type::I32);

        let combined = builder.binary(BinaryOp::AddInt32, i32_mul, f32_to_i32, Type::I32);

        builder.loop_(Some("loop"), combined, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        loop_body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_select_with_all_types() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Test select with i32
        let cond1 = builder.const_(Literal::I32(1));
        let i32_true = builder.const_(Literal::I32(100));
        let i32_false = builder.const_(Literal::I32(200));
        let select_i32 = builder.select(cond1, i32_true, i32_false, Type::I32);

        // Test select with i64
        let c150 = builder.const_(Literal::I32(150));
        let cond2 = builder.binary(BinaryOp::GtSInt32, select_i32, c150, Type::I32);
        let i64_true = builder.const_(Literal::I64(1000));
        let i64_false = builder.const_(Literal::I64(2000));
        let select_i64 = builder.select(cond2, i64_true, i64_false, Type::I64);

        // Test select with f32
        let c1500 = builder.const_(Literal::I64(1500));
        let cond3 = builder.binary(BinaryOp::LtSInt64, select_i64, c1500, Type::I32);
        let f32_true = builder.const_(Literal::F32(std::f32::consts::PI));
        let f32_false = builder.const_(Literal::F32(2.71));
        let select_f32 = builder.select(cond3, f32_true, f32_false, Type::F32);

        // Convert back to i32
        builder.unary(UnaryOp::TruncSFloat32ToInt32, select_f32, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_stress_all_bitwise_operations() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

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
        builder.binary(BinaryOp::XorInt32, r1, shr_u, Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}

#[test]
fn test_maximum_expression_complexity() {
    use crate::wasm::binary_reader::BinaryReader;
    use crate::wasm::binary_writer::BinaryWriter;

    let mut module = Module::new();
    module.add_memory(crate::ir::module::Memory {
        name: "mem".to_string(),
        limits: crate::ir::module::MemoryLimits {
            initial: 1,
            maximum: None,
        },
        kind: crate::ir::module::MemoryKind::Local,
    });

    let body = {
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Combine: nested blocks + loops + ifs + all operation types + memory ops + conversions
        let i32_start = builder.const_(Literal::I32(1));
        let i64_val = builder.const_(Literal::I64(100));

        // Memory operation
        let addr = builder.const_(Literal::I32(0));
        let c10 = builder.const_(Literal::I32(10));
        let stored_val = builder.binary(BinaryOp::AddInt32, i32_start, c10, Type::I32);
        let store_op = builder.store(MemoryId::from_u32(0), 4, 0, 4, addr, stored_val);

        // Load and process
        let load_addr = builder.const_(Literal::I32(0));
        let loaded = builder.load(MemoryId::from_u32(0), 4, false, 0, 2, load_addr, Type::I32);

        // Float operations
        let loaded_f32 = builder.unary(UnaryOp::ConvertSInt32ToFloat32, loaded, Type::F32);
        let f32_processed = builder.unary(UnaryOp::SqrtFloat32, loaded_f32, Type::F32);
        let back_to_i32 = builder.unary(UnaryOp::TruncSFloat32ToInt32, f32_processed, Type::I32);

        // Comparison and select
        let c5 = builder.const_(Literal::I32(5));
        let condition = builder.binary(BinaryOp::GtSInt32, back_to_i32, c5, Type::I32);
        let c999 = builder.const_(Literal::I64(999));
        let selected = builder.select(condition, i64_val, c999, Type::I64);

        // Bitwise operations
        let conv_f64 = builder.unary(UnaryOp::ConvertSInt64ToFloat64, selected, Type::F64);
        let i64_as_i32 = builder.unary(UnaryOp::TruncSFloat64ToInt32, conv_f64, Type::I32);
        let c4 = builder.const_(Literal::I32(4));
        let rotated = builder.binary(BinaryOp::RotLInt32, i64_as_i32, c4, Type::I32);

        // Final block
        builder.block(None::<&str>, vec![store_op, rotated], Type::I32)
    };

    module.add_function(Function::new(
        "test".to_string(),
        Type::NONE,
        Type::I32,
        vec![],
        body,
    ));

    let mut writer = BinaryWriter::new();
    let bytes = writer.write_module(&module).expect("Failed to write");

    let mut reader = BinaryReader::new();
    let parsed = reader.parse_module(&bytes).expect("Failed to parse");

    assert_eq!(parsed.functions.len(), 1);
}
