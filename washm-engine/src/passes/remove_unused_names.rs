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
                    used_names: HashSet::new(),
                };
                visitor.visit(body, arena);
            }
        }
    }
}

struct NameOptimizerVisitor {
    used_names: HashSet<String>,
}

impl Visitor for NameOptimizerVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = &arena[*expr_id].kind;
        match kind {
            ExpressionKind::Block { .. } | ExpressionKind::Loop { .. } => {
                // Visit children first
                self.visit_children(expr_id, arena);

                // Now check if the name of this block/loop was used
                if let ExpressionKind::Block { ref mut name, .. }
                | ExpressionKind::Loop { ref mut name, .. } = arena[*expr_id].kind
                {
                    if let Some(n) = name.clone() {
                        if !self.used_names.contains(&n) {
                            *name = None;
                        } else {
                            // Name was used by a break inside, but it shouldn't propagate outside
                            self.used_names.remove(&n);
                        }
                    }
                }
            }
            ExpressionKind::Break { name, .. } => {
                self.used_names.insert(name.clone());
                self.visit_children(expr_id, arena);
            }
            ExpressionKind::Switch { names, default, .. } => {
                for n in names {
                    self.used_names.insert(n.clone());
                }
                self.used_names.insert(default.clone());
                self.visit_children(expr_id, arena);
            }
            _ => self.visit_children(expr_id, arena),
        }
    }
}
