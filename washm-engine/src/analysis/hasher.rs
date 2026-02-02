use crate::ir::expression::{ExprRef, ExpressionKind};
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

    pub fn hash_expr(&mut self, expr: ExprRef<'a>) {
        self.visit(expr);
    }
}

impl<'a, H: Hasher> ReadOnlyVisitor<'a> for DeepHasher<'a, H> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        // Hash the kind discriminant
        std::mem::discriminant(&expr.kind).hash(self.hasher);
        expr.type_.hash(self.hasher);

        match &expr.kind {
            ExpressionKind::Const(lit) => match lit {
                Literal::I32(v) => v.hash(self.hasher),
                Literal::I64(v) => v.hash(self.hasher),
                Literal::F32(v) => v.to_bits().hash(self.hasher),
                Literal::F64(v) => v.to_bits().hash(self.hasher),
                Literal::V128(v) => v.hash(self.hasher),
            },
            ExpressionKind::LocalGet { index }
            | ExpressionKind::LocalSet { index, .. }
            | ExpressionKind::LocalTee { index, .. } => {
                index.hash(self.hasher);
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

        // Children are visited by default implementation of `visit` which calls `visit_children`
        // But `visit` calls `visit_expression` then `visit_children`.
        // So we just need to ensure `visit_children` traverses in order.
        // `ReadOnlyVisitor::visit` does this.
    }
}
