use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

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

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let targeted_ids = Self::collect_targets(arena, *body);
                let mut merger = BlockMerger { targeted_ids };
                merger.visit(body, arena);
            }
        }
    }
}

impl MergeBlocks {
    fn collect_targets(
        arena: &IdArena<ExpressionMarker, Expression>,
        expr_id: ExpressionId,
    ) -> std::collections::HashSet<ExpressionId> {
        struct TargetCollector<'a> {
            #[allow(dead_code)]
            arena: &'a IdArena<ExpressionMarker, Expression>,
            targets: std::collections::HashSet<ExpressionId>,
        }
        impl<'a> crate::ir::visitor::ReadOnlyVisitor for TargetCollector<'a> {
            fn visit_expression(
                &mut self,
                expr: ExpressionId,
                arena: &IdArena<ExpressionMarker, Expression>,
            ) {
                match &arena[expr].kind {
                    ExpressionKind::Break { target, .. } => {
                        self.targets.insert(*target);
                    }
                    ExpressionKind::Switch {
                        targets, default, ..
                    } => {
                        for t in targets {
                            self.targets.insert(*t);
                        }
                        self.targets.insert(*default);
                    }
                    _ => {}
                }
                self.visit_children(expr, arena);
            }
        }
        let mut collector = TargetCollector {
            arena,
            targets: std::collections::HashSet::new(),
        };
        use crate::ir::visitor::ReadOnlyVisitor;
        collector.visit(expr_id, arena);
        collector.targets
    }
}

struct BlockMerger {
    targeted_ids: std::collections::HashSet<ExpressionId>,
}

impl Visitor for BlockMerger {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Bottom-up: visit children first
        self.visit_children(expr, arena);

        let mut flattened_list = None;

        // Look for nested blocks that can be flattened
        if let ExpressionKind::Block { list, .. } = &arena[*expr].kind {
            let has_nesting = list.iter().any(|&child| {
                if let ExpressionKind::Block {
                    name: inner_name, ..
                } = &arena[child].kind
                {
                    inner_name.is_none() && !self.targeted_ids.contains(&child)
                } else {
                    false
                }
            });

            if has_nesting {
                let mut new_list = Vec::new();
                for &child in list.iter() {
                    if let ExpressionKind::Block {
                        name: None,
                        list: inner_list,
                    } = &arena[child].kind
                    {
                        if !self.targeted_ids.contains(&child) {
                            for &inner_child in inner_list.iter() {
                                new_list.push(inner_child);
                            }
                        } else {
                            new_list.push(child);
                        }
                    } else {
                        new_list.push(child);
                    }
                }
                flattened_list = Some(new_list);
            }
        }

        if let Some(new_list) = flattened_list {
            if let ExpressionKind::Block { list, .. } = &mut arena[*expr].kind {
                *list = new_list;
            }
        }

        // Blockify: pull code out of blocks that are operands
        self.blockify(expr, arena);
    }
}

impl BlockMerger {
    fn blockify(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Only if it's NOT a block itself (already handled by flattening)
        if matches!(arena[*expr_id].kind, ExpressionKind::Block { .. }) {
            return;
        }

        // Try to find a child that is a name-less block.
        // We start with the most common cases.
        let mut block_to_pull_id: Option<ExpressionId> = None;

        match &arena[*expr_id].kind {
            ExpressionKind::Unary { value, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &arena[*value].kind {
                    block_to_pull_id = Some(*value);
                }
            }
            ExpressionKind::Binary { left, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &arena[*left].kind {
                    block_to_pull_id = Some(*left);
                }
            }
            ExpressionKind::Drop { value } => {
                if let ExpressionKind::Block { name: None, .. } = &arena[*value].kind {
                    block_to_pull_id = Some(*value);
                }
            }
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                if let ExpressionKind::Block { name: None, .. } = &arena[*value].kind {
                    block_to_pull_id = Some(*value);
                }
            }
            _ => {}
        }

        if let Some(block_id) = block_to_pull_id {
            // Need to be careful with borrowing.
            // We'll clone fields if needed.
            let block_kind = std::mem::replace(&mut arena[block_id].kind, ExpressionKind::Nop);

            if let ExpressionKind::Block { mut list, name } = block_kind {
                if list.len() > 1 {
                    // Conservative safety: do not pull statements out of a child block if
                    // that block (or any descendant) contains control-flow constructs
                    // such as Break, Loop, Return, Switch, or If. Pulling those
                    // statements out can reorder control flow (observed with loops).
                    fn contains_control_flow(
                        arena: &IdArena<ExpressionMarker, Expression>,
                        id: ExpressionId,
                        seen: &mut std::collections::HashSet<ExpressionId>,
                    ) -> bool {
                        if !seen.insert(id) {
                            return false;
                        }
                        let expr = &arena[id];
                        match &expr.kind {
                            ExpressionKind::Break { .. }
                            | ExpressionKind::Loop { .. }
                            | ExpressionKind::Return { .. }
                            | ExpressionKind::Switch { .. }
                            | ExpressionKind::If { .. } => true,
                            ExpressionKind::Block { list, .. } => {
                                for &c in list {
                                    if contains_control_flow(arena, c, seen) {
                                        return true;
                                    }
                                }
                                false
                            }
                            _ => {
                                let mut found = false;
                                expr.kind.for_each_child(|c| {
                                    if contains_control_flow(arena, c, seen) {
                                        found = true;
                                    }
                                });
                                found
                            }
                        }
                    }

                    let mut skip_pull = false;
                    for &inner in &list {
                        let mut seen = std::collections::HashSet::new();
                        if contains_control_flow(arena, inner, &mut seen) {
                            skip_pull = true;
                            break;
                        }
                    }

                    if skip_pull {
                        // Put it back (preserve name)
                        arena[block_id].kind = ExpressionKind::Block { name, list };
                    } else {
                        let mut new_list = Vec::new();
                        // Take everything except the last
                        for _ in 0..list.len() - 1 {
                            new_list.push(list.remove(0));
                        }

                        // The last one stays in the position where the block was.
                        let last_id = list[0];
                        let last_type = arena[last_id].type_;
                        let last_kind =
                            std::mem::replace(&mut arena[last_id].kind, ExpressionKind::Nop);

                        // Update the block item to be the last expression instead
                        arena[block_id].type_ = last_type;
                        arena[block_id].kind = last_kind;

                        // Now wrap 'expr' in a new block; preserve the pulled block's name if present
                        let old_type = arena[*expr_id].type_;
                        let old_kind =
                            std::mem::replace(&mut arena[*expr_id].kind, ExpressionKind::Nop);

                        let wrapped_expr = arena.alloc(Expression {
                            kind: old_kind,
                            type_: old_type,
                        });
                        new_list.push(wrapped_expr);

                        arena[*expr_id].kind = ExpressionKind::Block {
                            name,
                            list: new_list,
                        };
                        arena[*expr_id].type_ = old_type;
                    }
                } else {
                    // Put it back (preserve name)
                    arena[block_id].kind = ExpressionKind::Block { name, list };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::module::Function;
    use crate::ir::ops::UnaryOp;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_merge_blocks_single_nested() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create: (block (block (i32.const 42)))
        let const_val = builder.const_(Literal::I32(42));
        let inner_block = builder.block("", vec![const_val], Type::I32);
        let outer_block = builder.block("", vec![inner_block], Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            outer_block,
        );
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should be flattened to single block
        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Const(_)
            ));
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_merge_blocks_preserves_non_nested() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create: (block (i32.const 1) (i32.const 2))
        let const1 = builder.const_(Literal::I32(1));
        let const2 = builder.const_(Literal::I32(2));

        let block = builder.block("", vec![const1, const2], Type::I32);

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], block);
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should remain unchanged
        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_merge_blocks_preserves_names() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create: (block $outer (block $inner (i32.const 42)))
        // Should not merge because names differ
        let const_val = builder.const_(Literal::I32(42));

        let inner_block = builder.block(Some("inner"), vec![const_val], Type::I32);
        let outer_block = builder.block(Some("outer"), vec![inner_block], Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            outer_block,
        );
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should NOT merge due to different names
        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, name } = &body.kind {
            assert_eq!(list.len(), 1);
            assert_eq!(*name, Some(String::from("outer")));
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Block { .. }
            ));
        }
    }

    #[test]
    fn test_merge_blocks_multiple_nested() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create:
        // (block
        //   (block (i32.const 1) (i32.const 2))
        //   (i32.const 3)
        //   (block (i32.const 4))
        // )
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let b1 = builder.block("", vec![c1, c2], Type::NONE);

        let c3 = builder.const_(Literal::I32(3));

        let c4 = builder.const_(Literal::I32(4));
        let b2 = builder.block("", vec![c4], Type::NONE);

        let main_block = builder.block("", vec![b1, c3, b2], Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            main_block,
        );
        module.add_function(func);

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 4);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Const(Literal::I32(1))
            ));
            assert!(matches!(
                module.expressions[list[1]].kind,
                ExpressionKind::Const(Literal::I32(2))
            ));
            assert!(matches!(
                module.expressions[list[2]].kind,
                ExpressionKind::Const(Literal::I32(3))
            ));
            assert!(matches!(
                module.expressions[list[3]].kind,
                ExpressionKind::Const(Literal::I32(4))
            ));
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_merge_blocks_blockify_unary() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create: (i32.eqz (block (call $foo) (i32.const 1)))
        let call = builder.nop(); // Simulation of a call (side effect)
        let c1 = builder.const_(Literal::I32(1));

        let block = builder.block("", vec![call, c1], Type::I32);

        let unary = builder.unary(UnaryOp::EqZInt32, block, Type::I32);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            unary,
        ));

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should become: (block (call $foo) (i32.eqz (i32.const 1)))
        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Nop
            )); // The call
            if let ExpressionKind::Unary { value, .. } = &module.expressions[list[1]].kind {
                assert!(matches!(
                    module.expressions[*value].kind,
                    ExpressionKind::Const(Literal::I32(1))
                ));
            } else {
                panic!(
                    "Expected Unary as second element of block, got {:?}",
                    module.expressions[list[1]].kind
                );
            }
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_merge_blocks_blockify_drop() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create: (drop (block (call $foo) (i32.const 1)))
        let call = builder.nop();
        let c1 = builder.const_(Literal::I32(1));

        let block = builder.block("", vec![call, c1], Type::I32);

        let drop = builder.drop(block);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            drop,
        ));

        let mut pass = MergeBlocks;
        pass.run(&mut module);

        // Should become: (block (call $foo) (drop (i32.const 1)))
        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();
        let body = &module.expressions[body_id];
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Nop
            ));
            assert!(matches!(
                module.expressions[list[1]].kind,
                ExpressionKind::Drop { .. }
            ));
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }
}
