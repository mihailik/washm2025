use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::Literal;

pub struct OptimizeForJS;

impl Pass for OptimizeForJS {
    fn name(&self) -> &str {
        "optimize-for-js"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut optimizer = JSOptimizer {
            allocator: module.allocator,
        };

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                optimizer.visit(body);
            }
        }
    }
}

struct JSOptimizer<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for JSOptimizer<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // 1. Unwrap singleton blocks with no name.
        if let ExpressionKind::Block { name, list } = &expr.kind {
            if name.is_none() && list.len() == 1 {
                let child = list[0];
                *expr = child;
                return;
            }
        }

        // 2. Remove Drop of Const.
        if let ExpressionKind::Drop { value } = &expr.kind {
            if let ExpressionKind::Const(_) = value.kind {
                // Replace with Nop
                *expr = Expression::nop(self.allocator);
                return;
            }
        }

        // 3. Optimize Select with constant condition.
        // Match reference to avoid moving out of expr
        if let ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } = &expr.kind
        {
            if let ExpressionKind::Const(lit) = &condition.kind {
                let is_true = match lit {
                    Literal::I32(v) => *v != 0,
                    Literal::I64(v) => *v != 0,
                    Literal::F32(v) => v.to_bits() != 0,
                    Literal::F64(v) => v.to_bits() != 0,
                    _ => false,
                };

                if is_true {
                    *expr = *if_true;
                } else {
                    *expr = *if_false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    // Helper to create drop since it might not be static on Expression
    fn make_drop<'a>(bump: &'a Bump, value: ExprRef<'a>) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Drop { value },
            type_: Type::NONE,
        }))
    }

    // Helper to create select
    fn make_select<'a>(
        bump: &'a Bump,
        cond: ExprRef<'a>,
        if_true: ExprRef<'a>,
        if_false: ExprRef<'a>,
        ty: Type,
    ) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Select {
                condition: cond,
                if_true,
                if_false,
            },
            type_: ty,
        }))
    }

    #[test]
    fn test_unwrap_singleton_block() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let const_expr = Expression::const_expr(&allocator, Literal::I32(42), Type::I32);
        let mut list = BumpVec::new_in(&allocator);
        list.push(const_expr);
        let block = Expression::block(&allocator, None, list, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(block),
        );
        module.add_function(func);

        let mut pass = OptimizeForJS;
        pass.run(&mut module);

        let func = module.get_function("test").unwrap();
        let body = func.body.unwrap();

        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(42))));
    }

    #[test]
    fn test_remove_drop_const() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let const_expr = Expression::const_expr(&allocator, Literal::I32(42), Type::I32);
        let drop_expr = make_drop(&allocator, const_expr);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(drop_expr),
        );
        module.add_function(func);

        let mut pass = OptimizeForJS;
        pass.run(&mut module);

        let func = module.get_function("test").unwrap();
        let body = func.body.unwrap();

        assert!(matches!(body.kind, ExpressionKind::Nop));
    }

    #[test]
    fn test_optimize_select() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let cond = Expression::const_expr(&allocator, Literal::I32(1), Type::I32);
        let true_val = Expression::const_expr(&allocator, Literal::I32(10), Type::I32);
        let false_val = Expression::const_expr(&allocator, Literal::I32(20), Type::I32);

        let select = make_select(&allocator, cond, true_val, false_val, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(select),
        );
        module.add_function(func);

        let mut pass = OptimizeForJS;
        pass.run(&mut module);

        let func = module.get_function("test").unwrap();
        let body = func.body.unwrap();

        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(10))));
    }
}
