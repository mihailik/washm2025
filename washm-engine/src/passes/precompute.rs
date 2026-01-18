use crate::analysis::evaluator::Evaluator;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// Precompute pass: Evaluates constant expressions at compile time
pub struct Precompute;

impl Pass for Precompute {
    fn name(&self) -> &str {
        "precompute"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let evaluator = Evaluator::new(module.allocator);
        let mut visitor = PrecomputeVisitor { evaluator };

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                visitor.visit(body);
            }
        }
    }
}

struct PrecomputeVisitor<'a> {
    evaluator: Evaluator<'a>,
}

impl<'a> Visitor<'a> for PrecomputeVisitor<'a> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up traversal: simplify children first
        self.visit_children(expr);

        // Try to fold current expression
        if let Some(lit) = self.evaluator.eval(*expr) {
            // Only replace if it's not already a constant
            if !matches!(expr.kind, ExpressionKind::Const(_)) {
                expr.kind = ExpressionKind::Const(lit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind, IrBuilder};
    use crate::ir::module::Function;
    use crate::ir::ops::BinaryOp;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_precompute_add() {
        let bump = Bump::new();
        let left = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let right = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let binary = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left,
                right,
            },
            type_: Type::I32,
        }));
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(binary),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = Precompute;
        pass.run(&mut module);
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(30))));
    }

    #[test]
    fn test_precompute_nested() {
        // (10 + 20) * 2 = 60
        let bump = Bump::new();
        let left = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let right = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let add = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left,
                right,
            },
            type_: Type::I32,
        }));
        let two = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(2)),
            type_: Type::I32,
        }));
        let mul = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::MulInt32,
                left: add,
                right: two,
            },
            type_: Type::I32,
        }));

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(mul));
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = Precompute;
        pass.run(&mut module);
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(60))));
    }

    #[test]
    fn test_precompute_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (i64.const 1) << (i64.const 33)
        let c1 = builder.const_(Literal::I64(1));
        let c33 = builder.const_(Literal::I64(33));
        let shl = builder.binary(BinaryOp::ShlInt64, c1, c33, Type::I64);

        let func = Function::new("test".to_string(), Type::NONE, Type::I64, vec![], Some(shl));

        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = Precompute;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I64(v)) if v == (1i64 << 33)));
    }
}
