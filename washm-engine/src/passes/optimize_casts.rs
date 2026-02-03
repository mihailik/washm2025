use crate::analysis::patterns::{MatchEnv, Pattern, PatternMatcher};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::ops::UnaryOp;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// OptimizeCasts pass: Removes redundant casts
pub struct OptimizeCasts {
    matcher: PatternMatcher,
}

impl Default for OptimizeCasts {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizeCasts {
    pub fn new() -> Self {
        let mut matcher = PatternMatcher::new();
        Self::register_rules(&mut matcher);
        Self { matcher }
    }

    fn register_rules(matcher: &mut PatternMatcher) {
        // wrap(extend_s(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::WrapInt64,
                Pattern::unary(UnaryOp::ExtendSInt32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // wrap(extend_u(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::WrapInt64,
                Pattern::unary(UnaryOp::ExtendUInt32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // demote(promote(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::DemoteFloat64,
                Pattern::unary(UnaryOp::PromoteFloat32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // trunc_s_f64(promote_f32(convert_s_i32(x))) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::TruncSFloat64ToInt32,
                Pattern::unary(
                    UnaryOp::PromoteFloat32,
                    Pattern::unary(UnaryOp::ConvertSInt32ToFloat32, Pattern::Var("x")),
                ),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // trunc_u_f64(promote_f32(convert_u_i32(x))) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::TruncUFloat64ToInt32,
                Pattern::unary(
                    UnaryOp::PromoteFloat32,
                    Pattern::unary(UnaryOp::ConvertUInt32ToFloat32, Pattern::Var("x")),
                ),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );
    }
}

impl Pass for OptimizeCasts {
    fn name(&self) -> &str {
        "optimize-casts"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut visitor = OptimizeCastsVisitor {
            matcher: &self.matcher,
        };

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct OptimizeCastsVisitor<'a> {
    matcher: &'a PatternMatcher,
}

impl<'a> Visitor for OptimizeCastsVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        if let Some(new_expr) = self.matcher.simplify(*expr_id, arena) {
            let new_node = std::mem::replace(&mut arena[new_expr].kind, ExpressionKind::Nop);
            arena[*expr_id].kind = new_node;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::{LocalId, Type};

    #[test]
    fn test_optimize_wrap_extend_s() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // wrap(extend_s(local.get 0)) -> local.get 0
        let x = builder.local_get(LocalId::from(0), Type::I32);
        let extend = builder.unary(UnaryOp::ExtendSInt32, x, Type::I64);
        let wrap = builder.unary(UnaryOp::WrapInt64, extend, Type::I32);

        let pass = OptimizeCasts::new();
        let mut visitor = OptimizeCastsVisitor {
            matcher: &pass.matcher,
        };

        let mut expr_id = wrap;
        visitor.visit(&mut expr_id, &mut arena);

        if let ExpressionKind::LocalGet { id, .. } = &arena[expr_id].kind {
            assert_eq!(id.index(), 0);
        } else {
            panic!("Expected LocalGet, got {:?}", arena[expr_id].kind);
        }
    }

    #[test]
    fn test_optimize_demote_promote() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // demote(promote(local.get 0)) -> local.get 0
        let x = builder.local_get(LocalId::from(0), Type::F32);
        let promote = builder.unary(UnaryOp::PromoteFloat32, x, Type::F64);
        let demote = builder.unary(UnaryOp::DemoteFloat64, promote, Type::F32);

        let mut expr_id = demote;
        let pass = OptimizeCasts::new();
        let mut visitor = OptimizeCastsVisitor {
            matcher: &pass.matcher,
        };

        visitor.visit(&mut expr_id, &mut arena);

        if let ExpressionKind::LocalGet { id, .. } = &arena[expr_id].kind {
            assert_eq!(id.index(), 0);
        } else {
            panic!("Expected LocalGet, got {:?}", arena[expr_id].kind);
        }
    }
}
