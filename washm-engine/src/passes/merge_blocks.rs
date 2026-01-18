use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use bumpalo::collections::Vec as BumpVec;

/// Merge Blocks pass: Combines consecutive blocks that can be merged
///
/// This pass looks for opportunities to flatten nested block structures
/// and merge sequential blocks into single blocks for simpler control flow.
///
/// Examples:
/// - (block (block ...)) => (block ...)
/// - Sequential blocks with no control flow => single block
pub struct MergeBlocks;

impl Pass for MergeBlocks {
    fn name(&self) -> &str {
        "merge-blocks"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut merger = BlockMerger { allocator };
                merger.visit(body);
            }
        }
    }
}

struct BlockMerger<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for BlockMerger<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up: visit children first
        self.visit_children(expr);

        // Look for nested blocks that can be flattened
        if let ExpressionKind::Block { list, .. } = &mut expr.kind {
            let has_nesting = list.iter().any(|child| {
                if let ExpressionKind::Block {
                    name: inner_name, ..
                } = &child.kind
                {
                    inner_name.is_none()
                } else {
                    false
                }
            });

            if has_nesting {
                let mut new_list = BumpVec::new_in(self.allocator);
                for child in list.iter() {
                    if let ExpressionKind::Block {
                        name: None,
                        list: inner_list,
                    } = &child.kind
                    {
                        for inner_child in inner_list.iter() {
                            new_list.push(*inner_child);
                        }
                    } else {
                        new_list.push(*child);
                    }
                }
                *list = new_list;
            }
        }

        // Blockify: pull code out of blocks that are operands
        self.blockify(expr);
    }
}

impl<'a> BlockMerger<'a> {
    fn blockify(&mut self, expr: &mut ExprRef<'a>) {
        // Only if it's NOT a block itself (already handled by flattening)
        if matches!(expr.kind, ExpressionKind::Block { .. }) {
            return;
        }

        // Try to find a child that is a name-less block.
        // We start with the most common cases.
        let mut block_to_pull: Option<&mut ExprRef<'a>> = None;

        match &mut expr.kind {
            ExpressionKind::Unary { value, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &value.kind {
                    block_to_pull = Some(value);
                }
            }
            ExpressionKind::Binary { left, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &left.kind {
                    block_to_pull = Some(left);
                }
            }
            ExpressionKind::Drop { value } => {
                if let ExpressionKind::Block { name: None, .. } = &value.kind {
                    block_to_pull = Some(value);
                }
            }
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &value.kind {
                    block_to_pull = Some(value);
                }
            }
            _ => {}
        }

        if let Some(block_ref) = block_to_pull {
            let kind = std::mem::replace(&mut block_ref.kind, ExpressionKind::Nop);
            if let ExpressionKind::Block { mut list, .. } = kind {
                if list.len() > 1 {
                    let mut new_list = BumpVec::new_in(self.allocator);
                    // Take everything except the last
                    for _ in 0..list.len() - 1 {
                        new_list.push(list.remove(0));
                    }

                    // The last one stays in the block
                    let mut last = list[0];
                    block_ref.type_ = last.type_;
                    block_ref.kind = std::mem::replace(&mut last.kind, ExpressionKind::Nop);

                    // Now wrap 'expr' in a new block
                    let old_type = expr.type_;
                    let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                    let wrapped_expr: ExprRef<'a> =
                        ExprRef::new(self.allocator.alloc(Expression {
                            kind: old_kind,
                            type_: old_type,
                        }));
                    new_list.push(wrapped_expr);

                    expr.kind = ExpressionKind::Block {
                        name: None,
                        list: new_list,
                    };
                    expr.type_ = old_type;
                } else {
                    // Put it back
                    block_ref.kind = ExpressionKind::Block { name: None, list };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::ops::UnaryOp;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_merge_blocks_single_nested() {
        let bump = Bump::new();

        // Create: (block (block (i32.const 42)))
        let const_val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);

        let mut inner_list = BumpVec::new_in(&bump);
        inner_list.push(const_val);
        let inner_block = Expression::block(&bump, None, inner_list, Type::I32);

        let mut outer_list = BumpVec::new_in(&bump);
        outer_list.push(inner_block);
        let outer_block = Expression::block(&bump, None, outer_list, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(outer_block),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should be flattened to single block
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1);
            assert!(matches!(list[0].kind, ExpressionKind::Const(_)));
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_merge_blocks_preserves_non_nested() {
        let bump = Bump::new();

        // Create: (block (i32.const 1) (i32.const 2))
        let const1 = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let const2 = Expression::const_expr(&bump, Literal::I32(2), Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(const1);
        list.push(const2);
        let block = Expression::block(&bump, None, list, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(block),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should remain unchanged
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_merge_blocks_preserves_names() {
        let bump = Bump::new();

        // Create: (block $outer (block $inner (i32.const 42)))
        // Should not merge because names differ
        let const_val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);

        let inner_name = bump.alloc_str("inner");
        let mut inner_list = BumpVec::new_in(&bump);
        inner_list.push(const_val);
        let inner_block = Expression::block(&bump, Some(inner_name), inner_list, Type::I32);

        let outer_name = bump.alloc_str("outer");
        let mut outer_list = BumpVec::new_in(&bump);
        outer_list.push(inner_block);
        let outer_block = Expression::block(&bump, Some(outer_name), outer_list, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(outer_block),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should NOT merge due to different names
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, name } = &body.kind {
            assert_eq!(list.len(), 1);
            assert_eq!(*name, Some("outer"));
            assert!(matches!(list[0].kind, ExpressionKind::Block { .. }));
        }
    }

    #[test]
    fn test_merge_blocks_multiple_nested() {
        let bump = Bump::new();

        // Create:
        // (block
        //   (block (i32.const 1) (i32.const 2))
        //   (i32.const 3)
        //   (block (i32.const 4))
        // )
        let c1 = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let c2 = Expression::const_expr(&bump, Literal::I32(2), Type::I32);
        let mut list1 = BumpVec::new_in(&bump);
        list1.push(c1);
        list1.push(c2);
        let b1 = Expression::block(&bump, None, list1, Type::NONE);

        let c3 = Expression::const_expr(&bump, Literal::I32(3), Type::I32);

        let c4 = Expression::const_expr(&bump, Literal::I32(4), Type::I32);
        let mut list2 = BumpVec::new_in(&bump);
        list2.push(c4);
        let b2 = Expression::block(&bump, None, list2, Type::NONE);

        let mut main_list = BumpVec::new_in(&bump);
        main_list.push(b1);
        main_list.push(c3);
        main_list.push(b2);
        let main_block = Expression::block(&bump, None, main_list, Type::NONE);

        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(main_block),
        ));

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 4);
            assert!(matches!(
                list[0].kind,
                ExpressionKind::Const(Literal::I32(1))
            ));
            assert!(matches!(
                list[1].kind,
                ExpressionKind::Const(Literal::I32(2))
            ));
            assert!(matches!(
                list[2].kind,
                ExpressionKind::Const(Literal::I32(3))
            ));
            assert!(matches!(
                list[3].kind,
                ExpressionKind::Const(Literal::I32(4))
            ));
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_merge_blocks_blockify_unary() {
        let bump = Bump::new();
        let builder = crate::ir::expression::IrBuilder::new(&bump);

        // Create: (i32.eqz (block (call $foo) (i32.const 1)))
        let call = builder.nop(); // Simulation of a call (side effect)
        let c1 = builder.const_(Literal::I32(1));

        let mut list = BumpVec::new_in(&bump);
        list.push(call);
        list.push(c1);
        let block = builder.block(None, list, Type::I32);

        let unary = builder.unary(UnaryOp::EqZInt32, block, Type::I32);

        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(unary),
        ));

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should become: (block (call $foo) (i32.eqz (i32.const 1)))
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(list[0].kind, ExpressionKind::Nop)); // The call
            if let ExpressionKind::Unary { value, .. } = &list[1].kind {
                assert!(matches!(value.kind, ExpressionKind::Const(Literal::I32(1))));
            } else {
                panic!(
                    "Expected Unary as second element of block, got {:?}",
                    list[1].kind
                );
            }
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_merge_blocks_blockify_drop() {
        let bump = Bump::new();
        let builder = crate::ir::expression::IrBuilder::new(&bump);

        // Create: (drop (block (call $foo) (i32.const 1)))
        let call = builder.nop();
        let c1 = builder.const_(Literal::I32(1));

        let mut list = BumpVec::new_in(&bump);
        list.push(call);
        list.push(c1);
        let block = builder.block(None, list, Type::I32);

        let drop = builder.drop(block);

        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(drop),
        ));

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should become: (block (call $foo) (drop (i32.const 1)))
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(list[0].kind, ExpressionKind::Nop));
            assert!(matches!(list[1].kind, ExpressionKind::Drop { .. }));
        } else {
            panic!("Expected Block");
        }
    }
}
