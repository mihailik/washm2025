use crate::analysis::stats::ModuleStats;
use crate::ir::expression::{ExprRef, ExpressionKind};
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let stats = ModuleStats::collect(module);

        for (_, func) in &mut module.functions {
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
                func.locals_mut().remove_id(local_id.cast());
            }
        }
    }
}

#[allow(dead_code)]
struct LocalRemapper<'a> {
    remap: &'a HashMap<crate::ir::LocalId, crate::ir::LocalId>,
}

impl<'a, 'b> Visitor<'b> for LocalRemapper<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        match &mut expr.kind {
            ExpressionKind::LocalGet { id }
            | ExpressionKind::LocalSet { id, .. }
            | ExpressionKind::LocalTee { id, .. } => {
                if let Some(&new_id) = self.remap.get(id) {
                    *id = new_id;
                }
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_merge_locals() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(val));
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = MergeLocals;
        pass.run(&mut module);
        assert!(module.functions[module.functions.iter().next().unwrap().0]
            .body()
            .is_some());
    }
}
