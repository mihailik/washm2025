use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::TypeId;
use crate::passes::Pass;
use std::collections::HashMap;

/// Reorders types in the module
pub struct ReorderTypes;

impl Pass for ReorderTypes {
    fn name(&self) -> &str {
        "reorder-types"
    }

    fn run(&mut self, module: &mut Module) {
        let remap = HashMap::new();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut visitor = ReorderTypesVisitor { remap: &remap };

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct ReorderTypesVisitor<'a> {
    remap: &'a HashMap<TypeId, TypeId>,
}

impl<'a> Visitor for ReorderTypesVisitor<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        match &mut arena[*expr_id].kind {
            ExpressionKind::CallIndirect { type_id, .. } => {
                if let Some(&new_id) = self.remap.get(type_id) {
                    *type_id = new_id;
                }
            }
            _ => {}
        }
    }
}
