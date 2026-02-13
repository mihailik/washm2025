use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};

pub trait Visitor {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_expression(expr, arena);
    }

    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr, arena);
    }

    fn visit_children(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut kind = std::mem::replace(&mut arena[*expr].kind, ExpressionKind::Nop);
        kind.for_each_child_mut(|child_id| self.visit(child_id, arena));
        arena[*expr].kind = kind;
    }
}

pub trait ReadOnlyVisitor {
    fn visit(&mut self, expr: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) {
        self.visit_expression(expr, arena);
    }

    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        _arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr, _arena);
    }

    fn visit_children(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        arena[expr]
            .kind
            .for_each_child(|child_id| self.visit(child_id, arena));
    }
}
