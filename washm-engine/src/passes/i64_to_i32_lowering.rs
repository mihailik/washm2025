use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::{Global, Module};
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use bumpalo::collections::Vec as BumpVec;
use bumpalo::Bump;

pub struct I64ToI32Lowering;

impl Pass for I64ToI32Lowering {
    fn name(&self) -> &str {
        "i64-to-i32-lowering"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Add a global to store the high 32 bits of an i64
        let global_name = "i64_to_i32_high_bits".to_string();
        let global_init = Expression::const_expr(module.allocator, Literal::I32(0), Type::I32);

        let global = Global {
            name: global_name,
            type_: Type::I32,
            mutable: true,
            init: global_init,
        };
        module.add_global(global);
        let global_index = (module.globals.len() - 1) as u32;

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut visitor = I64LoweringVisitor {
                    allocator: module.allocator,
                    global_index,
                };
                visitor.visit(body);
            }
        }
    }
}

struct I64LoweringVisitor<'a> {
    allocator: &'a Bump,
    global_index: u32,
}

impl<'a> Visitor<'a> for I64LoweringVisitor<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Handle ConstInt64
        let replacement = if let ExpressionKind::Const(Literal::I64(val)) = expr.kind {
            let low = val as i32;
            let high = (val >> 32) as i32;

            let const_high = Expression::const_expr(self.allocator, Literal::I32(high), Type::I32);

            let set_global = Expression::new(
                self.allocator,
                ExpressionKind::GlobalSet {
                    index: self.global_index,
                    value: const_high,
                },
                Type::NONE,
            );

            let const_low = Expression::const_expr(self.allocator, Literal::I32(low), Type::I32);

            let mut list = BumpVec::new_in(self.allocator);
            list.push(set_global);
            list.push(const_low);

            // Expression::block returns ExprRef
            Some(Expression::block(self.allocator, None, list, Type::I32))
        } else {
            None
        };

        if let Some(new_expr) = replacement {
            *expr = new_expr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_i64_to_i32_lowering_const() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        // Function returning i64 const
        let val: i64 = 0x123456789ABCDEF0u64 as i64;
        let const_expr = Expression::const_expr(&allocator, Literal::I64(val), Type::I64);

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(const_expr),
        );
        module.add_function(func);

        let mut pass = I64ToI32Lowering;
        pass.run(&mut module);

        let func = module.get_function("test_func").unwrap();
        let body = func.body.unwrap();

        match body.kind {
            ExpressionKind::Block { ref list, .. } => {
                assert_eq!(list.len(), 2);
                match list[0].kind {
                    ExpressionKind::GlobalSet { index, ref value } => {
                        assert_eq!(index, 0); // First global
                        match value.kind {
                            ExpressionKind::Const(Literal::I32(v)) => {
                                assert_eq!(v, (val >> 32) as i32);
                            }
                            _ => panic!("Expected Const for global set value"),
                        }
                    }
                    _ => panic!("Expected GlobalSet"),
                }
                match list[1].kind {
                    ExpressionKind::Const(Literal::I32(v)) => {
                        assert_eq!(v, val as i32);
                    }
                    _ => panic!("Expected Const for result"),
                }
            }
            _ => panic!("Expected Block"),
        }
    }
}
