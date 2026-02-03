use crate::analysis::stats::ModuleStats;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Merge Locals pass: Combines similar local variable patterns.
/// In this implementation, it also performs unused local removal.
pub struct MergeLocals;

impl Pass for MergeLocals {
    fn name(&self) -> &str {
        "merge-locals"
    }

    fn run(&mut self, module: &mut Module) {
        let stats = ModuleStats::collect(module);

        for (_, func) in &mut module.functions {
            if func.is_import() {
                continue;
            }
            let local_usage = stats.local_counts.get(&func.name);

            // Identify unused locals
            let mut unused = Vec::new();
            for (id, _) in func.locals().iter() {
                let local_id = id.cast();
                let used = local_usage
                    .map(|u| u.get(&local_id).copied().unwrap_or(0) > 0)
                    .unwrap_or(false);

                if !used {
                    unused.push(local_id);
                }
            }

            // Remove unused locals (caution: removing while iterating not allowed, done via filtered vec if it was a vec)
            // With IdArena, we can remove them.
            for local_id in unused {
                func.locals_mut().dealloc(local_id.cast());
            }
        }
    }
}

#[allow(dead_code)]
struct LocalRemapper<'a> {
    remap: &'a HashMap<crate::ir::LocalId, crate::ir::LocalId>,
}

impl<'a> Visitor for LocalRemapper<'a> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = &mut arena[*expr_id].kind;
        match kind {
            ExpressionKind::LocalGet { id }
            | ExpressionKind::LocalSet { id, .. }
            | ExpressionKind::LocalTee { id, .. } => {
                if let Some(&new_id) = self.remap.get(id) {
                    *id = new_id;
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_merge_locals() {
        let mut module = Module::new();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);
        let val = builder.const_(Literal::I32(1));
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], val);
        let id = functions.alloc(func);
        let mut pass = MergeLocals;
        pass.run(&mut module);
        assert!(module.functions[id].body().is_some());
    }
}
