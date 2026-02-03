use crate::analysis::usage::UsageTracker;
use crate::ir::module::{ExportKind, Module};
use crate::passes::Pass;

pub struct RemoveUnusedModuleElements;

impl Pass for RemoveUnusedModuleElements {
    fn name(&self) -> &str {
        "RemoveUnusedModuleElements"
    }

    fn run(&mut self, module: &mut Module) {
        let tracker = UsageTracker::analyze(module);

        // 1. Remove unused elements from arenas
        module
            .functions
            .retain(|id, _| tracker.functions.contains(&id));
        module.globals.retain(|id, _| tracker.globals.contains(&id));
        module.tables.retain(|id, _| tracker.tables.contains(&id));
        module
            .memories
            .retain(|id, _| tracker.memories.contains(&id));

        // 2. Filter elements and data segments
        module
            .elements
            .retain(|id, _| tracker.element_segments.contains(&id));
        module
            .data
            .retain(|id, _| tracker.data_segments.contains(&id));

        // 3. Types pruning
        module.types.retain(|id, _| tracker.types.contains(&id));

        // 4. Update references in the module
        // Exports
        module.exports.retain(|export| match export.kind {
            ExportKind::Function(id) => tracker.functions.contains(&id),
            ExportKind::Global(id) => tracker.globals.contains(&id),
            ExportKind::Table(id) => tracker.tables.contains(&id),
            ExportKind::Memory(id) => tracker.memories.contains(&id),
        });

        // Start
        if let Some(start_id) = module.start {
            if !tracker.functions.contains(&start_id) {
                module.start = None;
            }
        }

        // Update all expressions in the module
        // (No remapping needed for stable IDs)

        for (_, elem) in &mut module.elements {
            elem.func_indices
                .retain(|id| tracker.functions.contains(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{ExportKind, Function, Global, GlobalKind, Module};
    use crate::ir::{Literal, Type, TypeId};

    #[test]
    fn test_remove_unused_functions() {
        let mut module = Module::new();

        // Func 2: Called by Func 1 (Used)
        let id2 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let func2 = Function::new(
                "called".to_string(),
                Type::NONE,
                Type::NONE,
                vec![],
                builder.nop(),
            );
            module.add_function(func2)
        };

        // Func 1: Exported (Used)
        let id1 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            // Body of Func 1 calls Func 2
            let call = builder.call(id2, vec![], Type::NONE, false);
            let func1 = Function::new("exported".to_string(), Type::NONE, Type::NONE, vec![], call);
            module.add_function(func1)
        };
        module.export_function(id1, "main".to_string());

        // Func 0: Unused
        {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let func0 = Function::new(
                "unused".to_string(),
                Type::NONE,
                Type::NONE,
                vec![],
                builder.nop(),
            );
            module.add_function(func0);
        }

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Func 0 removed. 1 stays. 2 stays.
        assert_eq!(module.functions.len(), 2);
        let mut ids = module.functions.ids();
        let f0_id = ids.next().unwrap();
        let f1_id = ids.next().unwrap();
        assert_eq!(module.functions[f0_id].name, "called");
        assert_eq!(module.functions[f1_id].name, "exported");

        // Check Export index updated
        assert_eq!(
            match module.exports[0].kind {
                ExportKind::Function(idx) => idx,
                _ => panic!("Expected Function export"),
            },
            f1_id
        );
    }

    #[test]
    fn test_remove_unused_globals() {
        let mut module = Module::new();

        // Global 0: Unused
        let init0 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(0))
        };
        let glob0 = Global {
            name: "unused".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init0 },
        };
        module.add_global(glob0);

        // Global 1: Used by Export
        let init1 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(1))
        };
        let glob1 = Global {
            name: "exported".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init1 },
        };
        let g1_id = module.add_global(glob1);
        module.export_global(g1_id, "g".to_string());

        // Global 2: Used by Func 0 (which is exported)
        let init2 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.const_(Literal::I32(2))
        };
        let glob2 = Global {
            name: "used_by_code".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init2 },
        };
        let g2_id = module.add_global(glob2);

        // Func using Global 2
        let user_id = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let get = builder.global_get(g2_id, Type::I32);
            let func = Function::new("user".to_string(), Type::NONE, Type::NONE, vec![], get);
            module.add_function(func)
        };
        module.export_function(user_id, "user".to_string());

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Global 0 removed. 1 -> 0. 2 -> 1.
        assert_eq!(module.globals.len(), 2);
        let mut ids = module.globals.ids();
        let g_exp_id = ids.next().unwrap();
        let g_code_id = ids.next().unwrap();
        assert_eq!(module.globals[g_exp_id].name, "exported");
        assert_eq!(module.globals[g_code_id].name, "used_by_code");

        // Check Export index updated
        assert_eq!(
            match module.exports.iter().find(|e| e.name == "g").unwrap().kind {
                ExportKind::Global(id) => id,
                _ => panic!("Expected Global export"),
            },
            g_exp_id
        );

        // Check Code index updated
        let user_f_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[user_f_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::GlobalGet { id } = &body.kind {
            assert_eq!(*id, g_code_id);
        } else {
            panic!("Expected GlobalGet, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_remove_unused_types() {
        let mut module = Module::new();

        // Add 3 types
        module.types.push(crate::ir::module::FuncType {
            params: Type::I32,
            results: Type::I32,
        }); // Type 0
        module.types.push(crate::ir::module::FuncType {
            params: Type::F64,
            results: Type::F64,
        }); // Type 1
        module.types.push(crate::ir::module::FuncType {
            params: Type::I64,
            results: Type::I64,
        }); // Type 2

        let nop = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };

        // Function 0 (Exported) uses Type 1
        let mut func = Function::new("f0".to_string(), Type::F64, Type::F64, vec![], nop);
        func.type_idx = Some(TypeId::from(1));
        let f0_id = module.add_function(func);
        module.export_function(f0_id, "f0".to_string());

        // Function 1 (Unused) uses Type 2
        let nop2 = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.nop()
        };
        let mut func2 = Function::new("f1".to_string(), Type::I64, Type::I64, vec![], nop2);
        func2.type_idx = Some(TypeId::from(2));
        module.add_function(func2);

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Function 1 removed. Only Function 0 remains.
        assert_eq!(module.functions.len(), 1);

        // Type 0 and Type 2 should be gone. Only Type 1 remains.
        assert_eq!(module.types.len(), 1);
        let mut type_ids = module.types.ids();
        let remaining_type_id = type_ids.next().unwrap();
        assert_eq!(module.types[remaining_type_id].params, Type::F64);

        // Function 0's type_idx should still point to the same type.
        let f0_id = module.functions.ids().next().unwrap();
        assert_eq!(module.functions[f0_id].type_idx, Some(remaining_type_id));
    }

    #[test]
    fn test_remove_unused_imports_gtm() {
        let mut module = Module::new();

        // Import 0: Function (Used)
        let f_id = module.add_function(Function::new_import(
            "f".to_string(),
            "m".to_string(),
            "f".to_string(),
            Type::NONE,
            Type::NONE,
        ));

        // Global 0 (Unused)
        module.add_global(crate::ir::module::Global {
            name: "g_unused".to_string(),
            type_: crate::ir::Type::I32,
            mutable: false,
            kind: crate::ir::module::GlobalKind::Import {
                module: "m".to_string(),
                field: "g_unused".to_string(),
            },
        });

        // Global 1 (Used)
        let g_used_id = module.add_global(crate::ir::module::Global {
            name: "g_used".to_string(),
            type_: crate::ir::Type::I32,
            mutable: false,
            kind: crate::ir::module::GlobalKind::Import {
                module: "m".to_string(),
                field: "g_used".to_string(),
            },
        });

        // Add code that uses Function Import and Global Import
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let call = builder.call(f_id, vec![], Type::NONE, false);
            let get = builder.global_get(g_used_id, Type::I32);
            builder.block("", vec![call, get], Type::I32)
        };

        let func = Function::new("main".to_string(), Type::NONE, Type::I32, vec![], body);
        let main_id = module.add_function(func);
        module.export_function(main_id, "main".to_string());

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Import should be kept because it is used.
        let imports: Vec<_> = module
            .functions
            .iter()
            .filter(|(_, f)| f.is_import())
            .collect();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].1.name, "f");
    }
}
