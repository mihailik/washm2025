use crate::ir::module::Module;
use crate::passes::Pass;

pub struct RemoveMemoryInit;

impl Pass for RemoveMemoryInit {
    fn name(&self) -> &str {
        "RemoveMemoryInit"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let start_id = module.start;

        // 1. Remove all data segments
        module.data.clear();

        // 2. Clear start function
        module.start = None;

        // 3. Remove the start function itself if it exists
        if let Some(id) = start_id {
            // Check if it's exported
            let is_exported = module
                .exports
                .iter()
                .any(|e| matches!(e.kind, crate::ir::module::ExportKind::Function(i) if i == id));

            if !is_exported {
                // Remove function
                module.functions.remove(id.index() as usize);

                // Remove from elements if present
                for segment in &mut module.elements {
                    segment.func_indices.retain(|&idx| idx != id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Id;
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
            memory_index: Id::from_raw_parts(0, 0),
            offset,
            data: vec![1, 2, 3],
        });

        // Set start function
        // We need a function first
        let func = Function::new("start".to_string(), Type::NONE, Type::NONE, vec![], None);
        let id = module.add_function(func);
        module.set_start(id);

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
