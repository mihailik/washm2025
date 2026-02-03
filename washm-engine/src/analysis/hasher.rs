use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::Literal;
use std::hash::{Hash, Hasher};

pub struct DeepHasher<'a, H: Hasher> {
    hasher: &'a mut H,
}

impl<'a, H: Hasher> DeepHasher<'a, H> {
    pub fn new(hasher: &'a mut H) -> Self {
        Self { hasher }
    }

    pub fn hash_expr(&mut self, expr: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) {
        self.visit(expr, arena);
    }
}

impl<'a, H: Hasher> ReadOnlyVisitor for DeepHasher<'a, H> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let expression = &arena[expr];
        // Hash the kind discriminant
        std::mem::discriminant(&expression.kind).hash(self.hasher);
        expression.type_.hash(self.hasher);

        match &expression.kind {
            ExpressionKind::Const(lit) => match lit {
                Literal::I32(v) => v.hash(self.hasher),
                Literal::I64(v) => v.hash(self.hasher),
                Literal::F32(v) => v.to_bits().hash(self.hasher),
                Literal::F64(v) => v.to_bits().hash(self.hasher),
                Literal::V128(v) => v.hash(self.hasher),
            },
            ExpressionKind::LocalGet { id, .. }
            | ExpressionKind::LocalSet { id, .. }
            | ExpressionKind::LocalTee { id, .. } => {
                id.hash(self.hasher);
            }
            ExpressionKind::GlobalGet { id } | ExpressionKind::GlobalSet { id, .. } => {
                id.hash(self.hasher);
            }
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                name.hash(self.hasher);
            }
            ExpressionKind::Break { name, .. } => {
                name.hash(self.hasher);
            }
            ExpressionKind::Call {
                target, is_return, ..
            } => {
                target.hash(self.hasher);
                is_return.hash(self.hasher);
            }
            ExpressionKind::Unary { op, .. } => {
                op.hash(self.hasher);
            }
            ExpressionKind::Binary { op, .. } => {
                op.hash(self.hasher);
            }
            // Add other fields as necessary
            _ => {}
        }
    }
}
