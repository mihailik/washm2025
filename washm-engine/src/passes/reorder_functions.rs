use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::FuncId;
use crate::passes::Pass;
use std::collections::HashMap;

/// Reorders functions in the module according to a specific order
pub struct ReorderFunctions {
    pub order: Vec<FuncId>,
}

impl Pass for ReorderFunctions {
    fn name(&self) -> &str {
        "reorder-functions"
    }

    fn run(&mut self, module: &mut Module) {
        // This is a complex pass that needs to remap all function references
        let remap = HashMap::new();
        // ... Reordering logic would go here ...

        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut visitor = ReorderFunctionsVisitor { remap: &remap };

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct ReorderFunctionsVisitor<'a> {
    remap: &'a HashMap<FuncId, FuncId>,
}

impl<'a> Visitor for ReorderFunctionsVisitor<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        if let ExpressionKind::Call { target, .. } = &mut arena[*expr_id].kind {
            if let Some(&new_id) = self.remap.get(target) {
                *target = new_id;
            }
        }
    }
}
