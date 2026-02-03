use crate::ir::module::Module;
use crate::passes::Pass;

pub struct RemoveMemoryInit;

impl Pass for RemoveMemoryInit {
    fn name(&self) -> &str {
        "RemoveMemoryInit"
    }

    fn run(&mut self, module: &mut Module) {
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
                module.functions.remove_id(id);

                // Remove from elements if present
                for (_, segment) in &mut module.elements {
                    segment.func_indices.retain(|&idx| idx != id);
                }
            }
        }
    }
}
