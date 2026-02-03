use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::FuncId;
use crate::passes::Pass;
use std::collections::HashMap;

/// Removes indirect calls and replaces them with direct calls where possible
#[derive(Default)]
pub struct RemoveImports {
    pub to_replace: HashMap<FuncId, FuncId>,
}

impl Pass for RemoveImports {
    fn name(&self) -> &str {
        "remove-imports"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut visitor = RemoveImportsVisitor {
            to_replace: &self.to_replace,
        };

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct RemoveImportsVisitor<'a> {
    to_replace: &'a HashMap<FuncId, FuncId>,
}

impl<'a> Visitor for RemoveImportsVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        if let ExpressionKind::Call { target, .. } = &mut arena[*expr_id].kind {
            if let Some(&new_id) = self.to_replace.get(target) {
                *target = new_id;
            }
        }
    }
}
