use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashSet;

/// Removes names from blocks and loops that are not targeted by any branch.
pub struct RemoveUnusedNames;

impl Pass for RemoveUnusedNames {
    fn name(&self) -> &str {
        "RemoveUnusedNames"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = NameOptimizerVisitor {
                    used_targets: HashSet::new(),
                };
                visitor.visit(body, arena);
            }
        }
    }
}

struct NameOptimizerVisitor {
    used_targets: HashSet<ExpressionId>,
}

impl Visitor for NameOptimizerVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[*expr_id].kind {
            ExpressionKind::Break { target, .. } => {
                self.used_targets.insert(*target);
            }
            ExpressionKind::Switch {
                targets, default, ..
            } => {
                for t in targets {
                    self.used_targets.insert(*t);
                }
                self.used_targets.insert(*default);
            }
            _ => {}
        }

        self.visit_children(expr_id, arena);

        let id = *expr_id;
        match &mut arena[id].kind {
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                if !self.used_targets.contains(&id) {
                    *name = None;
                } else {
                    self.used_targets.remove(&id);
                }
            }
            _ => {}
        }
    }
}
