use crate::analysis::patterns::{MatchEnv, Pattern, PatternMatcher};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
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
            |env: &MatchEnv, _arena| env.get("x").copied(),
        );

        // f32.reinterpret_i32(i32.reinterpret_f32(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt32,
                Pattern::unary(UnaryOp::ReinterpretFloat32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _arena| env.get("x").copied(),
        );

        // i64.reinterpret_f64(f64.reinterpret_i64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretFloat64,
                Pattern::unary(UnaryOp::ReinterpretInt64, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _arena| env.get("x").copied(),
        );

        // f64.reinterpret_i64(i64.reinterpret_f64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt64,
                Pattern::unary(UnaryOp::ReinterpretFloat64, Pattern::Var("x")),
            ),
            |env: &MatchEnv, _arena| env.get("x").copied(),
        );
    }
}

impl Pass for AvoidReinterprets {
    fn name(&self) -> &str {
        "avoid-reinterprets"
    }

    fn run(&mut self, module: &mut Module) {
        let mut visitor = AvoidReinterpretsVisitor {
            matcher: &self.matcher,
        };

        let (expressions, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, expressions);
            }
        }
    }
}

struct AvoidReinterpretsVisitor<'a> {
    matcher: &'a PatternMatcher,
}

impl<'a> Visitor for AvoidReinterpretsVisitor<'a> {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr, arena);
        if let Some(new_expr) = self.matcher.simplify(*expr, arena) {
            *expr = new_expr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::{LocalId, Type};

    #[test]
    fn test_avoid_reinterprets_roundtrip() {
        let mut module = Module::new();
        let (expressions, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(expressions);

        // i32.reinterpret_f32(f32.reinterpret_i32(local.get 0)) -> local.get 0
        let x = builder.local_get(LocalId::from(0), Type::I32);
        let f = builder.unary(UnaryOp::ReinterpretFloat32, x, Type::F32);
        let i = builder.unary(UnaryOp::ReinterpretInt32, f, Type::I32);

        let mut expr_id = i;
        let pass = AvoidReinterprets::new();
        let mut visitor = AvoidReinterpretsVisitor {
            matcher: &pass.matcher,
        };

        visitor.visit(&mut expr_id, expressions);

        let expr = &expressions[expr_id];
        if let ExpressionKind::LocalGet { id, .. } = &expr.kind {
            assert_eq!(id.index(), 0);
        } else {
            panic!("Expected LocalGet, got {:?}", expr.kind);
        }
    }
}
