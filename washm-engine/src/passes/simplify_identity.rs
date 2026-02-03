use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::visitor::Visitor;
use crate::ir::Literal;
use crate::passes::Pass;

pub struct SimplifyIdentity;

impl Pass for SimplifyIdentity {
    fn name(&self) -> &str {
        "SimplifyIdentity"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut simplifier = IdentitySimplifier;
                simplifier.visit(body, arena);
            }
        }
    }
}

struct IdentitySimplifier;

impl Visitor for IdentitySimplifier {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = arena[*expr].kind.clone();
        if let Some(new_expr) = simplify_identity(kind, arena) {
            *expr = new_expr;
        }
    }
}

fn simplify_identity(
    kind: ExpressionKind,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> Option<ExpressionId> {
    match kind {
        ExpressionKind::Unary { op, value } => {
            match op {
                UnaryOp::EqZInt32 => {
                    if let ExpressionKind::Unary {
                        op: UnaryOp::EqZInt32,
                        value: inner,
                    } = &arena[value].kind
                    {
                        return Some(*inner); // eqz(eqz(x)) -> x (for i32 bools)
                    }
                }
                _ => {}
            }
        }
        ExpressionKind::Binary { op, left, right } => match op {
            BinaryOp::AddInt32
            | BinaryOp::SubInt32
            | BinaryOp::OrInt32
            | BinaryOp::XorInt32
            | BinaryOp::ShlInt32
            | BinaryOp::ShrSInt32
            | BinaryOp::ShrUInt32 => {
                if is_const_zero(right, arena) {
                    return Some(left);
                }
                if (op == BinaryOp::AddInt32 || op == BinaryOp::OrInt32 || op == BinaryOp::XorInt32)
                    && is_const_zero(left, arena)
                {
                    return Some(right);
                }
            }
            BinaryOp::MulInt32 => {
                if is_const_one(right, arena) {
                    return Some(left);
                }
                if is_const_one(left, arena) {
                    return Some(right);
                }
            }
            _ => {}
        },
        _ => {}
    }
    None
}

fn is_const_zero(id: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) -> bool {
    if let ExpressionKind::Const(Literal::I32(0)) = &arena[id].kind {
        true
    } else {
        false
    }
}

fn is_const_one(id: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) -> bool {
    if let ExpressionKind::Const(Literal::I32(1)) = &arena[id].kind {
        true
    } else {
        false
    }
}
