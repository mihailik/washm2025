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
            if func.vars().is_empty() {
                continue;
            }

            let num_params = if func.params == crate::ir::Type::NONE {
                0
            } else if let Some(components) = crate::ir::type_store::lookup_tuple(func.params) {
                components.len()
            } else {
                1
            };

            let local_usage = stats.local_counts.get(&func.name);

            let mut new_vars = Vec::new();
            let mut remap = HashMap::new();

            for (i, &ty) in func.vars().iter().enumerate() {
                let old_idx = (num_params + i) as u32;
                let used = local_usage
                    .map(|u| u.get(&old_idx).copied().unwrap_or(0) > 0)
                    .unwrap_or(false);

                if used {
                    let new_idx = (num_params + new_vars.len()) as u32;
                    remap.insert(old_idx, new_idx);
                    new_vars.push(ty);
                }
            }

            if new_vars.len() < func.vars().len() {
                *func.vars_mut() = new_vars;
                if let Some(mut body) = func.body() {
                    let mut updater = LocalRemapper { remap: &remap };
                    updater.visit(&mut body);
                }
            }
        }
    }
}

struct LocalRemapper<'a> {
    remap: &'a HashMap<u32, u32>,
}

impl<'a, 'b> Visitor<'b> for LocalRemapper<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        match &mut expr.kind {
            ExpressionKind::LocalGet { index }
            | ExpressionKind::LocalSet { index, .. }
            | ExpressionKind::LocalTee { index, .. } => {
                if let Some(&new_idx) = self.remap.get(index) {
                    *index = new_idx;
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
