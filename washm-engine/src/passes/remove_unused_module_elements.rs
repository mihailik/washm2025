use crate::analysis::usage::UsageTracker;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, GlobalKind, ImportKind, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

pub struct RemoveUnusedModuleElements;

impl Pass for RemoveUnusedModuleElements {
    fn name(&self) -> &str {
        "RemoveUnusedModuleElements"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let tracker = UsageTracker::analyze(module);

        // 1. Determine which elements to keep and compute remapping

        // Functions (Imports + Defined)
        let mut func_remap = HashMap::new();
        let mut new_functions = Vec::new();
        let mut new_imports = Vec::new();

        let mut current_func_idx = 0;
        let mut next_func_idx = 0;

        let old_imports = std::mem::take(&mut module.imports);

        for import in old_imports {
            match import.kind {
                ImportKind::Function(_, _) => {
                    if tracker.functions.contains(&import.name) {
                        func_remap.insert(current_func_idx, next_func_idx);
                        next_func_idx += 1;
                        new_imports.push(import);
                    }
                    current_func_idx += 1;
                }
                ImportKind::Memory(..) => {
                    if tracker.memories {
                        new_imports.push(import);
                    }
                }
                ImportKind::Table(..) => {
                    if tracker.tables {
                        new_imports.push(import);
                    }
                }
            }
        }

        let old_functions = std::mem::take(&mut module.functions);
        for func in old_functions {
            if tracker.functions.contains(&func.name) {
                func_remap.insert(current_func_idx, next_func_idx);
                next_func_idx += 1;
                new_functions.push(func);
            }
            current_func_idx += 1;
        }
        module.functions = new_functions;
        module.imports = new_imports;

        // Globals (deallocate unused)
        let to_remove: Vec<_> = module
            .globals
            .iter()
            .filter(|(id, _)| !tracker.globals.contains(&id.cast()))
            .map(|(id, _)| id)
            .collect();
        for id in to_remove {
            module.globals.dealloc(id);
        }

        // Memories & Tables
        if !tracker.memories {
            module.memory = None;
            module.data.clear();
        }
        if !tracker.tables {
            module.table = None;
            module.elements.clear();
        }

        // Segments remapping (Data)
        let mut data_remap = HashMap::new();
        if tracker.memories {
            let old_data = std::mem::take(&mut module.data);
            let mut next_data_idx = 0;
            for (i, data) in old_data.into_iter().enumerate() {
                if tracker.data_segments.contains(&(i as u32)) {
                    data_remap.insert(i as u32, next_data_idx);
                    next_data_idx += 1;
                    module.data.push(data);
                }
            }
        }

        // Segments remapping (Elements)
        let mut elem_remap = HashMap::new();
        if tracker.tables {
            let old_elements = std::mem::take(&mut module.elements);
            let mut next_elem_idx = 0;
            for (i, elem) in old_elements.into_iter().enumerate() {
                if tracker.element_segments.contains(&(i as u32)) {
                    elem_remap.insert(i as u32, next_elem_idx);
                    next_elem_idx += 1;
                    module.elements.push(elem);
                }
            }
        }

        // --- Types Pruning and Remapping ---
        let mut new_types = Vec::new();
        let mut type_remap = HashMap::new();
        let old_types = std::mem::take(&mut module.types);
        for (i, ty) in old_types.into_iter().enumerate() {
            if tracker.types.contains(&(i as u32)) {
                type_remap.insert(i as u32, new_types.len() as u32);
                new_types.push(ty);
            }
        }
        module.types = new_types;

        for func in &mut module.functions {
            if let Some(type_idx) = &mut func.type_idx {
                if let Some(&new_idx) = type_remap.get(type_idx) {
                    *type_idx = new_idx;
                }
            }
        }

        // 2. Update references in the module

        // Exports
        module.exports.retain_mut(|export| match &mut export.kind {
            ExportKind::Function(index) => {
                if let Some(&new_idx) = func_remap.get(index) {
                    *index = new_idx;
                    true
                } else {
                    false
                }
            }
            ExportKind::Global(id) => tracker.globals.contains(id),
            ExportKind::Table(_) => tracker.tables,
            ExportKind::Memory(_) => tracker.memories,
        });

        // Start
        if let Some(start_idx) = module.start {
            if let Some(&new_idx) = func_remap.get(&start_idx) {
                module.start = Some(new_idx);
            } else {
                module.start = None;
            }
        }

        // Update all expressions in the module
        let mut updater = IndexUpdater {
            data_remap: &data_remap,
            elem_remap: &elem_remap,
            type_remap: &type_remap,
        };

        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                updater.visit(&mut body);
            }
        }

        for (_, global) in &mut module.globals {
            if let GlobalKind::Local { init } = &mut global.kind {
                updater.visit(init);
            }
        }

        for elem in &mut module.elements {
            updater.visit(&mut elem.offset);
            elem.func_indices.retain_mut(|idx| {
                if let Some(&new_idx) = func_remap.get(idx) {
                    *idx = new_idx;
                    true
                } else {
                    false
                }
            });
        }

        for data in &mut module.data {
            updater.visit(&mut data.offset);
        }
    }
}

struct IndexUpdater<'a> {
    data_remap: &'a HashMap<u32, u32>,
    elem_remap: &'a HashMap<u32, u32>,
    type_remap: &'a HashMap<u32, u32>,
}

impl<'a, 'b> Visitor<'b> for IndexUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);

        // Update the expression's own type
        self.remap_type(&mut expr.type_);

        match &mut expr.kind {
            ExpressionKind::GlobalGet { .. } | ExpressionKind::GlobalSet { .. } => {
                // GlobalId is stable, no remapping needed!
            }
            ExpressionKind::MemoryInit { segment, .. } | ExpressionKind::DataDrop { segment } => {
                if let Some(&new_idx) = self.data_remap.get(segment) {
                    *segment = new_idx;
                }
            }
            ExpressionKind::TableInit { segment, .. } | ExpressionKind::ElemDrop { segment } => {
                if let Some(&new_idx) = self.elem_remap.get(segment) {
                    *segment = new_idx;
                }
            }
            ExpressionKind::CallIndirect { type_, .. } => {
                self.remap_type(type_);
            }
            _ => {}
        }
    }
}

impl<'a> IndexUpdater<'a> {
    fn remap_type(&self, ty: &mut Type) {
        if let Some(id) = ty.signature_id() {
            if let Some(&new_id) = self.type_remap.get(&id) {
                *ty = Type::from_signature_id(new_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Global, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_remove_unused_functions() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);
        let builder = IrBuilder::new(&bump);

        // Func 0: Unused
        let func0 = Function::new("unused".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func0);

        // Func 1: Exported (Used)
        let func1 = Function::new("exported".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func1);
        module.export_function(1, "main".to_string());

        // Func 2: Called by Func 1 (Used)
        let func2 = Function::new("called".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func2);

        // Body of Func 1 calls Func 2
        let call = builder.call("called", BumpVec::new_in(&bump), Type::NONE, false);
        module.functions[1].body = Some(call);

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Func 0 removed. 1 stays. 2 stays.
        assert_eq!(module.functions.len(), 2);
        assert_eq!(module.functions[0].name, "exported");
        assert_eq!(module.functions[1].name, "called");

        // Check Export index updated
        assert_eq!(
            match module.exports[0].kind {
                ExportKind::Function(idx) => idx,
                _ => panic!("Expected Function export"),
            },
            0
        );
    }

    #[test]
    fn test_remove_unused_globals() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);
        let builder = IrBuilder::new(&bump);

        // Global 0: Unused
        let init0 = builder.const_(Literal::I32(0));
        let glob0 = Global {
            name: "unused".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init0 },
        };
        module.add_global(glob0);

        // Global 1: Used by Export
        let init1 = builder.const_(Literal::I32(1));
        let glob1 = Global {
            name: "exported".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init1 },
        };
        let g1_id = module.add_global(glob1);
        module.export_global(g1_id, "g".to_string());

        // Global 2: Used by Func 0 (which is exported)
        let init2 = builder.const_(Literal::I32(2));
        let glob2 = Global {
            name: "used_by_code".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: GlobalKind::Local { init: init2 },
        };
        let g2_id = module.add_global(glob2);

        // Func using Global 2
        let get = builder.global_get(g2_id, Type::I32);
        let func = Function::new(
            "user".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(get),
        );
        module.add_function(func);
        module.export_function(0, "user".to_string());

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Global 0 removed. 1 -> 0. 2 -> 1.
        assert_eq!(module.globals.len(), 2);
        let mut ids = module.globals.ids();
        let g0_id = ids.next().unwrap();
        let g1_id = ids.next().unwrap();
        assert_eq!(module.globals[g0_id].name, "exported");
        assert_eq!(module.globals[g1_id].name, "used_by_code");

        // Check Export index updated
        assert_eq!(
            match module.exports.iter().find(|e| e.name == "g").unwrap().kind {
                ExportKind::Global(id) => id,
                _ => panic!("Expected Global export"),
            },
            g0_id.cast()
        );

        // Check Code index updated
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::GlobalGet { id } = &body.kind {
            assert_eq!(*id, g1_id.cast());
        } else {
            panic!("Expected GlobalGet");
        }
    }

    #[test]
    fn test_remove_unused_types() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

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

        // Function 0 (Exported) uses Type 1
        let mut func = Function::new("f0".to_string(), Type::F64, Type::F64, vec![], None);
        func.type_idx = Some(1);
        module.add_function(func);
        module.export_function(0, "f0".to_string());

        // Function 1 (Unused) uses Type 2
        let mut func2 = Function::new("f1".to_string(), Type::I64, Type::I64, vec![], None);
        func2.type_idx = Some(2);
        module.add_function(func2);

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Function 1 removed. Only Function 0 remains.
        assert_eq!(module.functions.len(), 1);

        // Type 0 and Type 2 should be gone. Only Type 1 remains.
        assert_eq!(module.types.len(), 1);
        assert_eq!(module.types[0].params, Type::F64);

        // Function 0's type_idx should be remapped from 1 to 0.
        assert_eq!(module.functions[0].type_idx, Some(0));
    }

    #[test]
    fn test_remove_unused_imports_gtm() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);
        let builder = IrBuilder::new(&bump);

        // Import 0: Function (Used)
        module.add_import(crate::ir::module::Import {
            module: "m".to_string(),
            name: "f".to_string(),
            kind: ImportKind::Function(Type::NONE, Type::NONE),
        });

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
        let call = builder.call("f", BumpVec::new_in(&bump), Type::NONE, false);
        let get = builder.global_get(g_used_id, Type::I32);

        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(call);
        block_list.push(get);
        let body = builder.block(None, block_list, Type::I32);

        let func = Function::new(
            "main".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        );
        module.add_function(func);
        module.export_function(1, "main".to_string()); // Func 1 is defined func

        let mut pass = RemoveUnusedModuleElements;
        pass.run(&mut module);

        // Import 1 should be gone.
        // new_imports: [Import 0 (f)]
        // (Global imports are tracked in module.globals, not module.imports)
        assert_eq!(module.imports.len(), 1);
        assert_eq!(module.imports[0].name, "f");
    }
}
