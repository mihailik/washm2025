use crate::ir::annotation::{Annotation, HighLevelType};
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use crate::ir::ops::BinaryOp;
use crate::ir::visitor::ReadOnlyVisitor;

/// A pass that identifies expressions that are likely boolean values.
pub struct IdentifyBooleans;

impl Default for IdentifyBooleans {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentifyBooleans {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        let mut booleans = Vec::new();

        for (_, func) in &module.functions {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let mut visitor = BooleanVisitor {
                    booleans: &mut booleans,
                };
                visitor.visit(*body, &module.expressions);
            }
        }

        for expr in booleans {
            module
                .annotations
                .insert(expr, Annotation::Type(HighLevelType::Bool));
        }
    }
}

struct BooleanVisitor<'a> {
    booleans: &'a mut Vec<ExpressionId>,
}

impl<'a> ReadOnlyVisitor for BooleanVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        if is_boolean_producer(expr, arena) {
            self.booleans.push(expr);
        }
    }
}

fn is_boolean_producer(
    expr: ExpressionId,
    arena: &crate::collections::id_arena::IdArena<
        crate::ir::expression::ExpressionMarker,
        crate::ir::expression::Expression,
    >,
) -> bool {
    match &arena[expr].kind {
        ExpressionKind::Binary { op, .. } => matches!(
            op,
            BinaryOp::EqInt32
                | BinaryOp::NeInt32
                | BinaryOp::LtSInt32
                | BinaryOp::LtUInt32
                | BinaryOp::GtSInt32
                | BinaryOp::GtUInt32
                | BinaryOp::LeSInt32
                | BinaryOp::LeUInt32
                | BinaryOp::GeSInt32
                | BinaryOp::GeUInt32
                | BinaryOp::EqInt64
                | BinaryOp::NeInt64
                | BinaryOp::LtSInt64
                | BinaryOp::LtUInt64
                | BinaryOp::GtSInt64
                | BinaryOp::GtUInt64
                | BinaryOp::LeSInt64
                | BinaryOp::LeUInt64
                | BinaryOp::GeSInt64
                | BinaryOp::GeUInt64
                | BinaryOp::EqFloat32
                | BinaryOp::NeFloat32
                | BinaryOp::LtFloat32
                | BinaryOp::GtFloat32
                | BinaryOp::LeFloat32
                | BinaryOp::GeFloat32
                | BinaryOp::EqFloat64
                | BinaryOp::NeFloat64
                | BinaryOp::LtFloat64
                | BinaryOp::GtFloat64
                | BinaryOp::LeFloat64
                | BinaryOp::GeFloat64
        ),
        ExpressionKind::Unary { op, .. } => matches!(
            op,
            crate::ir::ops::UnaryOp::EqZInt32 | crate::ir::ops::UnaryOp::EqZInt64
        ),
        _ => false,
    }
}
