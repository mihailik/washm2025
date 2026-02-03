use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Literal;
use crate::passes::Pass;

pub struct Simplify;

impl Pass for Simplify {
    fn name(&self) -> &str {
        "Simplify"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut simplifier = Simplifier;
                simplifier.visit(body, arena);
            }
        }
    }
}

struct Simplifier;

impl Visitor for Simplifier {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = arena[*expr].kind.clone();
        if let Some(new_expr) = simplify_expression(kind, arena) {
            *expr = new_expr;
        }
    }
}

fn simplify_expression(
    kind: ExpressionKind,
    arena: &mut IdArena<ExpressionMarker, Expression>,
) -> Option<ExpressionId> {
    match kind {
        ExpressionKind::If {
            name: _,
            condition,
            if_true,
            if_false,
        } => {
            if let ExpressionKind::Const(lit) = arena[condition].kind {
                let cond_value = match lit {
                    Literal::I32(v) => v != 0,
                    Literal::I64(v) => v != 0,
                    _ => return None,
                };

                if cond_value {
                    Some(if_true)
                } else {
                    if_false
                }
            } else {
                None
            }
        }
        ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } => {
            if let ExpressionKind::Const(lit) = arena[condition].kind {
                let cond_value = match lit {
                    Literal::I32(v) => v != 0,
                    Literal::I64(v) => v != 0,
                    _ => return None,
                };

                Some(if cond_value { if_true } else { if_false })
            } else {
                None
            }
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn are_expressions_equal(
    left: ExpressionId,
    right: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    if left == right {
        return true;
    }

    let left_expr = &arena[left];
    let right_expr = &arena[right];

    if left_expr.type_ != right_expr.type_ {
        return false;
    }

    match (&left_expr.kind, &right_expr.kind) {
        (ExpressionKind::Const(l), ExpressionKind::Const(r)) => l == r,
        (ExpressionKind::LocalGet { id: l }, ExpressionKind::LocalGet { id: r }) => l == r,
        _ => false,
    }
}
