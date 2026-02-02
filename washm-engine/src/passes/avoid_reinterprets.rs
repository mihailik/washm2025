use crate::analysis::patterns::{MatchEnv, Pattern, PatternMatcher};
use crate::ir::expression::ExprRef;
use crate::ir::module::Module;
use crate::ir::ops::UnaryOp;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// AvoidReinterprets pass: Removes redundant reinterpret operations
pub struct AvoidReinterprets {
    matcher: PatternMatcher,
}

impl Default for AvoidReinterprets {
    fn default() -> Self {
        Self::new()
    }
}

impl AvoidReinterprets {
    pub fn new() -> Self {
        let mut matcher = PatternMatcher::new();
        Self::register_rules(&mut matcher);
        Self { matcher }
    }

    fn register_rules(matcher: &mut PatternMatcher) {
        // i32.reinterpret_f32(f32.reinterpret_i32(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretFloat32,
                Pattern::unary(UnaryOp::ReinterpretInt32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // f32.reinterpret_i32(i32.reinterpret_f32(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt32,
                Pattern::unary(UnaryOp::ReinterpretFloat32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // i64.reinterpret_f64(f64.reinterpret_i64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretFloat64,
                Pattern::unary(UnaryOp::ReinterpretInt64, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // f64.reinterpret_i64(i64.reinterpret_f64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt64,
                Pattern::unary(UnaryOp::ReinterpretFloat64, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );
    }
}

impl Pass for AvoidReinterprets {
    fn name(&self) -> &str {
        "avoid-reinterprets"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut visitor = AvoidReinterpretsVisitor {
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

struct AvoidReinterpretsVisitor<'a, 'b> {
    matcher: &'b PatternMatcher,
    allocator: &'a bumpalo::Bump,
}

impl<'a, 'b> Visitor<'a> for AvoidReinterpretsVisitor<'a, 'b> {
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
    use crate::ir::{LocalId, Type};
    use bumpalo::Bump;

    #[test]
    fn test_avoid_reinterprets_roundtrip() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // i32.reinterpret_f32(f32.reinterpret_i32(local.get 0)) -> local.get 0
        let x = builder.local_get(LocalId::from(0), Type::I32);
        let f = builder.unary(UnaryOp::ReinterpretInt32, x, Type::F32);
        let i = builder.unary(UnaryOp::ReinterpretFloat32, f, Type::I32);

        let mut expr_ref = i;
        let pass = AvoidReinterprets::new();
        let mut visitor = AvoidReinterpretsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        visitor.visit(&mut expr_ref);

        if let ExpressionKind::LocalGet { id, .. } = &expr_ref.kind {
            assert_eq!(id.index(), 0);
        } else {
            panic!("Expected LocalGet, got {:?}", expr_ref.kind);
        }
    }
}
