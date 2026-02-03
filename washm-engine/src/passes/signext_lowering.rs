use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::ops::UnaryOp;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct SignextLowering;

impl Pass for SignextLowering {
    fn name(&self) -> &str {
        "SignextLowering"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = SignextVisitor;
                visitor.visit(body, arena);
            }
        }
    }
}

struct SignextVisitor;

impl Visitor for SignextVisitor {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = &arena[*expr].kind;
        if let ExpressionKind::Unary {
            op: UnaryOp::ExtendSInt32,
            value,
        } = kind
        {
            // This is just an example of lowering. Actual implementation depends on what we lower TO.
            // For now, let's just leave it as is or do a placeholder replacement.
            let _value = *value;
        }
    }
}
