use crate::ir::expression::{ExpressionKind, IrBuilder};
use crate::ir::module::{ImportKind, Module};
use crate::ir::ops::BinaryOp;
use crate::ir::Type;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

pub struct StackCheck;

impl Pass for StackCheck {
    fn name(&self) -> &str {
        "stack-check"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Find __stack_pointer and __stack_limit globals
        let mut sp_index = None;
        let mut limit_index = None;

        let mut index: u32 = 0;
        for import in &module.imports {
            if let ImportKind::Global(_, _) = import.kind {
                if import.name == "__stack_pointer" {
                    sp_index = Some(index);
                } else if import.name == "__stack_limit" {
                    limit_index = Some(index);
                }
                index += 1;
            }
        }

        for global in &module.globals {
            if global.name == "__stack_pointer" {
                sp_index = Some(index);
            } else if global.name == "__stack_limit" {
                limit_index = Some(index);
            }
            index += 1;
        }

        if let (Some(sp), Some(limit)) = (sp_index, limit_index) {
            for func in &mut module.functions {
                if let Some(body) = func.body {
                    let builder = IrBuilder::new(module.allocator);

                    // Create check: if (sp < limit) unreachable
                    let sp_get = builder.global_get(sp, Type::I32);
                    let limit_get = builder.global_get(limit, Type::I32);
                    // Assuming stack grows down, check if SP < Limit (overflow)
                    // Using LtU32 for unsigned comparison of addresses
                    let condition =
                        builder.binary(BinaryOp::LtUInt32, sp_get, limit_get, Type::I32);
                    let trap = builder.unreachable();
                    let check = builder.if_(condition, trap, None, Type::NONE);

                    // Prepend check to body
                    let new_body = match body.kind {
                        ExpressionKind::Block { name, ref list } => {
                            let mut new_list =
                                BumpVec::with_capacity_in(list.len() + 1, module.allocator);
                            new_list.push(check);
                            for item in list.iter() {
                                new_list.push(*item);
                            }
                            builder.block(name, new_list, body.type_)
                        }
                        _ => {
                            let mut list = BumpVec::new_in(module.allocator);
                            list.push(check);
                            list.push(body);
                            builder.block(None, list, body.type_)
                        }
                    };

                    func.body = Some(new_body);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::{Function, Global};
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_stack_check_run() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        // Add globals
        let sp_global = Global {
            name: "__stack_pointer".to_string(),
            type_: Type::I32,
            mutable: true,
            init: Expression::const_expr(&allocator, Literal::I32(1000), Type::I32),
        };
        module.add_global(sp_global);

        let limit_global = Global {
            name: "__stack_limit".to_string(),
            type_: Type::I32,
            mutable: false,
            init: Expression::const_expr(&allocator, Literal::I32(100), Type::I32),
        };
        module.add_global(limit_global);

        let block = allocator.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: BumpVec::new_in(&allocator),
            },
            type_: Type::NONE,
        });

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block)),
        );
        module.add_function(func);

        let mut pass = StackCheck;
        pass.run(&mut module);

        let func = module.get_function("test_func").unwrap();
        let body = func.body.unwrap();

        // Verify check was added
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert!(!list.is_empty());
            let check = list[0];
            if let ExpressionKind::If {
                condition, if_true, ..
            } = &check.kind
            {
                if let ExpressionKind::Binary { op, left, .. } = &condition.kind {
                    assert!(matches!(op, BinaryOp::LtUInt32));
                    if let ExpressionKind::GlobalGet { index } = left.kind {
                        assert_eq!(index, 0); // SP is first global (index 0 if no imports)
                    } else {
                        panic!("Expected GlobalGet for SP");
                    }
                } else {
                    panic!("Expected Binary LtU32");
                }
                assert!(matches!(if_true.kind, ExpressionKind::Unreachable));
            } else {
                panic!("Expected If");
            }
        } else {
            panic!("Expected Block");
        }
    }
}
