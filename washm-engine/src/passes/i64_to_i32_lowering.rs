use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{Global, GlobalId, GlobalKind, Module};
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use crate::passes::Pass;

pub struct I64ToI32Lowering;

impl Pass for I64ToI32Lowering {
    fn name(&self) -> &str {
        "i64-to-i32-lowering"
    }

    fn run(&mut self, module: &mut Module) {
        // Add a global to store the high 32 bits of an i64
        let global_name = "i64_to_i32_high_bits".to_string();
        let global_init = IrBuilder::new(&mut module.expressions).const_(Literal::I32(0));

        let global_id = module.add_global(Global {
            name: global_name,
            type_: Type::I32,
            mutable: true,
            kind: GlobalKind::Local { init: global_init },
        });

        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions {
            if let Some(mut body) = func.body_mut() {
                let mut visitor = I64LoweringVisitor { global_id };
                visitor.visit(&mut body, arena);
            }
        }
    }
}

struct I64LoweringVisitor {
    global_id: GlobalId,
}

impl Visitor for I64LoweringVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Handle ConstInt64
        let replacement = if let ExpressionKind::Const(Literal::I64(val)) = arena[*expr_id].kind {
            let low = val as i32;
            let high = (val >> 32) as i32;

            let mut builder = IrBuilder::new(arena);
            let const_high = builder.const_(Literal::I32(high));

            let set_global = builder.global_set(self.global_id, const_high);

            let const_low = builder.const_(Literal::I32(low));

            let list = vec![set_global, const_low];

            Some(builder.block("", list, Type::I32))
        } else {
            None
        };

        if let Some(new_expr) = replacement {
            *expr_id = new_expr;
        }
        self.visit_children(expr_id, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_i64_to_i32_lowering_const() {
        let mut module = Module::new();

        // Function returning i64 const
        let val: i64 = 0x123456789ABCDEF0u64 as i64;
        let const_expr = IrBuilder::new(&mut module.expressions).const_(Literal::I64(val));

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            const_expr,
        );
        module.add_function(func);

        let mut pass = I64ToI32Lowering;
        pass.run(&mut module);

        let func = module.functions.iter().next().unwrap().1;
        let body_id = func.body().unwrap();
        let body = &module.expressions[body_id];

        match &body.kind {
            ExpressionKind::Block { list, .. } => {
                assert_eq!(list.len(), 2);
                match module.expressions[list[0]].kind {
                    ExpressionKind::GlobalSet { id, value } => {
                        assert_eq!(id, module.globals.iter().next().unwrap().0); // First global
                        match module.expressions[value].kind {
                            ExpressionKind::Const(Literal::I32(v)) => {
                                assert_eq!(v, (val >> 32) as i32);
                            }
                            _ => panic!("Expected Const for global set value"),
                        }
                    }
                    _ => panic!("Expected GlobalSet"),
                }
                match module.expressions[list[1]].kind {
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
