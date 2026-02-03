use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct Rereloop;

impl Pass for Rereloop {
    fn name(&self) -> &str {
        "rereloop"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = RereloopVisitor;
                visitor.visit(body, arena);
            }
        }
    }
}

struct RereloopVisitor;
impl Visitor for RereloopVisitor {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr, arena);
    }
}
