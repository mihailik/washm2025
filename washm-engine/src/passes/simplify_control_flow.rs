use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct SimplifyControlFlow;

impl Pass for SimplifyControlFlow {
    fn name(&self) -> &str {
        "SimplifyControlFlow"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                let mut simplifier = ControlFlowSimplifier;
                simplifier.visit(body, arena);
            }
        }
    }
}

struct ControlFlowSimplifier;

impl Visitor for ControlFlowSimplifier {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = &arena[*expr].kind;
        if let ExpressionKind::Block { list, .. } = kind {
            if list.is_empty() {
                arena[*expr].kind = ExpressionKind::Nop;
            }
        }
    }
}
