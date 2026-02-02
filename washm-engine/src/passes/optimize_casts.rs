use crate::analysis::patterns::{MatchEnv, Pattern, PatternMatcher};
use crate::ir::expression::ExprRef;
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut visitor = OptimizeCastsVisitor {
            matcher: &self.matcher,
            allocator: module.allocator,
        };

        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body);
            }
        }
    }
}

struct OptimizeCastsVisitor<'a, 'b> {
    matcher: &'b PatternMatcher,
    allocator: &'a bumpalo::Bump,
}

impl<'a, 'b> Visitor<'a> for OptimizeCastsVisitor<'a, 'b> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_children(expr);
        if let Some(new_expr) = self.matcher.simplify(*expr, self.allocator) {
            *expr = new_expr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_optimize_wrap_extend_s() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // wrap(extend_s(local.get 0)) -> local.get 0
        let x = builder.local_get(0, Type::I32);
        let extend = builder.unary(UnaryOp::ExtendSInt32, x, Type::I64);
        let wrap = builder.unary(UnaryOp::WrapInt64, extend, Type::I32);

        let pass = OptimizeCasts::new();
        let mut visitor = OptimizeCastsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = wrap;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }

    #[test]
    fn test_optimize_demote_promote() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // demote(promote(local.get 0)) -> local.get 0
        let x = builder.local_get(0, Type::F32);
        let promote = builder.unary(UnaryOp::PromoteFloat32, x, Type::F64);
        let demote = builder.unary(UnaryOp::DemoteFloat64, promote, Type::F32);

        let mut expr_ref = demote;
        let pass = OptimizeCasts::new();
        let mut visitor = OptimizeCastsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }
}
