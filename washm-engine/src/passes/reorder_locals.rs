use crate::ir::module::Module;
use crate::passes::Pass;

/// Reorders locals within each function to group them by type (for better compression).
pub struct ReorderLocals;

impl Pass for ReorderLocals {
    fn name(&self) -> &str {
        "ReorderLocals"
    }

    fn run(&mut self, module: &mut Module) {
        for (_, func) in &mut module.functions {
            if let crate::ir::module::FunctionKind::Local {
                var_ids, locals, ..
            } = &mut func.kind
            {
                if var_ids.is_empty() {
                    continue;
                }

                // Group by type for better compression in the final binary
                var_ids.sort_by_key(|&id| {
                    locals
                        .get(id.cast())
                        .map(|l| l.type_)
                        .unwrap_or(crate::ir::Type::NONE)
                });
            }
        }
    }
}
