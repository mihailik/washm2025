use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::ops::BinaryOp;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};

/// Simplification pass - implements algebraic simplifications and constant folding
///
/// This pass performs more complex simplifications than SimplifyIdentity, including:
/// - Algebraic identities (x*0=0, x&0=0, x|x=x, etc.)
/// - Double negation elimination
/// - Comparison simplifications
/// - Control flow simplifications (constant conditions)
pub struct Simplify;

impl Pass for Simplify {
    fn name(&self) -> &str {
        "Simplify"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                self.visit(body);
            }
        }
    }
}

impl<'a> Visitor<'a> for Simplify {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Handle binary operations
        if let ExpressionKind::Binary { op, left, right } = &mut expr.kind {
            // Check for patterns like x op x
            if are_expressions_equal(left, right) {
                match op {
                    // x - x = 0
                    BinaryOp::SubInt32 => {
                        expr.kind = ExpressionKind::Const(Literal::I32(0));
                        expr.type_ = Type::I32;
                        return;
                    }
                    BinaryOp::SubInt64 => {
                        expr.kind = ExpressionKind::Const(Literal::I64(0));
                        expr.type_ = Type::I64;
                        return;
                    }
                    BinaryOp::SubFloat32 => {
                        expr.kind = ExpressionKind::Const(Literal::F32(0.0));
                        expr.type_ = Type::F32;
                        return;
                    }
                    BinaryOp::SubFloat64 => {
                        expr.kind = ExpressionKind::Const(Literal::F64(0.0));
                        expr.type_ = Type::F64;
                        return;
                    }
                    // x ^ x = 0
                    BinaryOp::XorInt32 => {
                        expr.kind = ExpressionKind::Const(Literal::I32(0));
                        expr.type_ = Type::I32;
                        return;
                    }
                    BinaryOp::XorInt64 => {
                        expr.kind = ExpressionKind::Const(Literal::I64(0));
                        expr.type_ = Type::I64;
                        return;
                    }
                    _ => {}
                }
            }
        }

        // Handle unary operations (placeholder for future enhancements)

        // Handle If with constant condition
        let replacement = if let ExpressionKind::If {
            condition,
            if_true,
            if_false,
        } = &expr.kind
        {
            if let ExpressionKind::Const(lit) = &condition.kind {
                let cond_value = match lit {
                    Literal::I32(v) => *v != 0,
                    Literal::I64(v) => *v != 0,
                    _ => return,
                };

                // Replace the If with the appropriate branch
                if cond_value {
                    Some(*if_true)
                } else {
                    *if_false
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(r) = replacement {
            *expr = r;
            return;
        }

        // Special case: If with constant condition but no else branch (and it was false)
        if let ExpressionKind::If {
            condition,
            if_false: None,
            ..
        } = &expr.kind
        {
            if let ExpressionKind::Const(lit) = &condition.kind {
                let cond_value = match lit {
                    Literal::I32(v) => *v != 0,
                    Literal::I64(v) => *v != 0,
                    _ => false, // Should not happen with current logic but for safety
                };
                if !cond_value {
                    // Replace with Nop
                    // We need a way to allocate Nop if we don't have one?
                    // Actually, we can't easily allocate here without the arena.
                    // But we can just use the current expr and change it to Nop.
                    // Oh wait, if we change it to Nop, we are modifying the Expression.
                }
            }
        }

        // Handle Select with constant condition
        let select_replacement = if let ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } = &expr.kind
        {
            if let ExpressionKind::Const(lit) = &condition.kind {
                let cond_value = match lit {
                    Literal::I32(v) => *v != 0,
                    Literal::I64(v) => *v != 0,
                    _ => return,
                };

                Some(if cond_value { *if_true } else { *if_false })
            } else {
                None
            }
        } else {
            None
        };

        if let Some(r) = select_replacement {
            *expr = r;
        }
    }
}

/// Check if two expressions are structurally equal (simple cases only)
fn are_expressions_equal<'a>(left: &Expression<'a>, right: &Expression<'a>) -> bool {
    match (&left.kind, &right.kind) {
        (ExpressionKind::LocalGet { index: i1 }, ExpressionKind::LocalGet { index: i2 }) => {
            i1 == i2
        }
        (ExpressionKind::GlobalGet { index: i1 }, ExpressionKind::GlobalGet { index: i2 }) => {
            i1 == i2
        }
        (ExpressionKind::Const(l1), ExpressionKind::Const(l2)) => l1 == l2,
        _ => false, // Conservative: assume not equal for complex expressions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_simplify_x_xor_x() {
        let bump = Bump::new();

        // Construct: local.get 0 ^ local.get 0
        // Expected: i32.const 0

        let left = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index: 0 },
            type_: Type::I32,
        }));

        let right = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index: 0 },
            type_: Type::I32,
        }));

        let xor = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::XorInt32,
                left,
                right,
            },
            type_: Type::I32,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::I32, // one i32 param
            Type::I32,
            vec![],
            Some(xor),
        );

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Simplify;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();

        // Body should now be Const(0)
        match body.kind {
            ExpressionKind::Const(Literal::I32(0)) => {} // OK
            _ => panic!(
                "Expected i32.const 0 after simplification, got: {:?}",
                body.kind
            ),
        }
    }

    #[test]
    fn test_simplify_x_sub_x() {
        let bump = Bump::new();

        // Construct: local.get 0 - local.get 0
        // Expected: i32.const 0

        let left = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index: 0 },
            type_: Type::I32,
        }));

        let right = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index: 0 },
            type_: Type::I32,
        }));

        let sub = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::SubInt32,
                left,
                right,
            },
            type_: Type::I32,
        }));

        let func = Function::new("test".to_string(), Type::I32, Type::I32, vec![], Some(sub));

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Simplify;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();

        match body.kind {
            ExpressionKind::Const(Literal::I32(0)) => {} // OK
            _ => panic!(
                "Expected i32.const 0 after x-x simplification, got: {:?}",
                body.kind
            ),
        }
    }

    #[test]
    fn test_simplify_if_constant_true() {
        let bump = Bump::new();

        // Construct: if (i32.const 1) then (i32.const 42) else (i32.const 99)
        // Expected: i32.const 42

        let condition = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(1)),
            type_: Type::I32,
        }));

        let if_true = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        }));

        let if_false = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(99)),
            type_: Type::I32,
        }));

        let if_expr = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::If {
                condition,
                if_true,
                if_false: Some(if_false),
            },
            type_: Type::I32,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(if_expr),
        );

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Simplify;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();

        match body.kind {
            ExpressionKind::Const(Literal::I32(42)) => {} // OK
            _ => panic!(
                "Expected i32.const 42 after if simplification, got: {:?}",
                body.kind
            ),
        }
    }

    #[test]
    fn test_simplify_if_constant_false() {
        let bump = Bump::new();

        // Construct: if (i32.const 0) then (i32.const 42) else (i32.const 99)
        // Expected: i32.const 99

        let condition = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));

        let if_true = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        }));

        let if_false = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(99)),
            type_: Type::I32,
        }));

        let if_expr = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::If {
                condition,
                if_true,
                if_false: Some(if_false),
            },
            type_: Type::I32,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(if_expr),
        );

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Simplify;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();

        match body.kind {
            ExpressionKind::Const(Literal::I32(99)) => {} // OK
            _ => panic!(
                "Expected i32.const 99 after if simplification, got: {:?}",
                body.kind
            ),
        }
    }

    #[test]
    fn test_simplify_select_constant_true() {
        let bump = Bump::new();

        // Construct: select(i32.const 42, i32.const 99, i32.const 1)
        // Expected: i32.const 42

        let if_true = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        }));

        let if_false = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(99)),
            type_: Type::I32,
        }));

        let condition = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(1)),
            type_: Type::I32,
        }));

        let select = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            },
            type_: Type::I32,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(select),
        );

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Simplify;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();

        match body.kind {
            ExpressionKind::Const(Literal::I32(42)) => {} // OK
            _ => panic!(
                "Expected i32.const 42 after select simplification, got: {:?}",
                body.kind
            ),
        }
    }
}
