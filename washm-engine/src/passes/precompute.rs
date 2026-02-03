use crate::analysis::evaluator::Evaluator;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// Precompute pass: Evaluates constant expressions at compile time
pub struct Precompute;

impl Pass for Precompute {
    fn name(&self) -> &str {
        "precompute"
    }

    fn run(&mut self, module: &mut Module) {
        let evaluator = Evaluator::new();
        let mut visitor = PrecomputeVisitor { evaluator };

        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct PrecomputeVisitor {
    evaluator: Evaluator,
}

impl Visitor for PrecomputeVisitor {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Bottom-up traversal: simplify children first
        self.visit_children(expr, arena);

        // Try to fold current expression
        if let Some(lit) = self.evaluator.eval(*expr, arena) {
            // Only replace if it's not already a constant
            if !matches!(arena[*expr].kind, ExpressionKind::Const(_)) {
                arena[*expr].kind = ExpressionKind::Const(lit);
                arena[*expr].type_ = lit.get_type();
            }
        }
    }
}
