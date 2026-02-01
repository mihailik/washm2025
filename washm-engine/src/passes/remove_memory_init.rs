use crate::ir::module::Module;
use crate::passes::Pass;

pub struct RemoveMemoryInit;

impl Pass for RemoveMemoryInit {
    fn name(&self) -> &str {
        "RemoveMemoryInit"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let start_idx = module.start;

        // 1. Remove all data segments
        module.data.clear();

        // 2. Clear start function
        module.start = None;

        // 3. Remove the start function itself if it exists
        if let Some(idx) = start_idx {
            // Check if it's exported
            let is_exported = module
                .exports
                .iter()
                .any(|e| matches!(e.kind, crate::ir::module::ExportKind::Function(i) if i == idx));

            if !is_exported {
                // Remove function and remap
                self.remove_function(module, idx);
            }
        }
    }
}

impl RemoveMemoryInit {
    fn remove_function(&self, module: &mut crate::ir::module::Module, index: u32) {
        if (index as usize) < module.functions.len() {
            module.functions.remove(index as usize);

            // Remap exports
            for export in &mut module.exports {
                if let crate::ir::module::ExportKind::Function(func_idx) = &mut export.kind {
                    if *func_idx > index {
                        *func_idx -= 1;
                    }
                }
            }

            // Remap elements
            for segment in &mut module.elements {
                for func_idx in &mut segment.func_indices {
                    if *func_idx > index {
                        *func_idx -= 1;
                    }
                }
            }

            // Remap start (not needed here since we just set it to None, but for completeness)
            if let Some(start) = &mut module.start {
                if *start > index {
                    *start -= 1;
                } else if *start == index {
                    module.start = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::module::{DataSegment, Function, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_remove_memory_init() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        // Add a data segment
        let offset = Expression::const_expr(&bump, Literal::I32(0), Type::I32);
        module.add_data_segment(DataSegment {
            memory_index: 0,
            offset,
            data: vec![1, 2, 3],
        });

        // Set start function
        // We need a function first
        let func = Function::new("start".to_string(), Type::NONE, Type::NONE, vec![], None);
        module.add_function(func);
        module.set_start(0);

        // Verify state before pass
        assert!(!module.data.is_empty());
        assert!(module.start.is_some());

        // Run pass
        let mut pass = RemoveMemoryInit;
        pass.run(&mut module);

        // Verify state after pass
        assert!(module.data.is_empty(), "Data segments should be removed");
        assert!(module.start.is_none(), "Start function should be unset");

        // Function should be removed (since it's not exported)
        assert_eq!(
            module.functions.len(),
            0,
            "Non-exported start function should be removed"
        );
    }
}
