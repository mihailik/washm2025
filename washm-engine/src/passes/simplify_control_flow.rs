use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct SimplifyControlFlow;

impl Pass for SimplifyControlFlow {
    fn name(&self) -> &str {
        "SimplifyControlFlow"
    }

    fn run(&mut self, module: &mut Module) {
        println!("PASS: SimplifyControlFlow");
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                let mut simplifier = ControlFlowSimplifier;
                simplifier.visit(body, arena);
            }
        }
    }
}

struct ControlFlowSimplifier;

impl Visitor for ControlFlowSimplifier {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Visit children first to simplify from bottom up
        self.visit_children(expr_id, arena);

        if let ExpressionKind::Block { list, name } = &arena[*expr_id].kind {
            if list.is_empty() && name.is_none() {
                arena[*expr_id].kind = ExpressionKind::Nop;
            } else {
                // Try to find an `if` that "dominates" the rest of the block and has
                // a branch that terminates. If so, flatten the `else` into the block.
                let mut new_list = Vec::new();
                let mut dirty = false;
                let list = list.clone();
                let block_name = name.clone();

                for &child_id in &list {
                    let mut candidate = None;
                    if let ExpressionKind::If {
                        condition,
                        if_true,
                        if_false: Some(if_false_id),
                        name: if_name,
                    } = &arena[child_id].kind
                    {
                        if self.is_terminator(*if_true, arena)
                            && !self.is_terminator(*if_false_id, arena)
                        {
                            candidate = Some((
                                *condition,
                                *if_true,
                                *if_false_id,
                                if_name.clone(),
                                arena[child_id].type_,
                            ));
                        }
                    }

                    if let Some((condition, if_true, if_false_id, if_name, if_type)) = candidate {
                        // Add the `if` without the `else` back to the list
                        let if_without_else = Expression::alloc(
                            arena,
                            ExpressionKind::If {
                                condition,
                                if_true,
                                if_false: None,
                                name: if_name,
                            },
                            if_type,
                        );
                        new_list.push(if_without_else);

                        // If the `else` branch is a block, spread its children
                        // into the parent block.
                        match &arena[if_false_id].kind {
                            ExpressionKind::Block {
                                list: inner_list,
                                name: None,
                            } => {
                                for &inner_child in inner_list {
                                    new_list.push(inner_child);
                                }
                            }
                            _ => {
                                new_list.push(if_false_id);
                            }
                        }
                        dirty = true;
                        continue;
                    }
                    new_list.push(child_id);
                }

                if dirty {
                    arena[*expr_id].kind = ExpressionKind::Block {
                        list: new_list,
                        name: block_name,
                    };
                }
            }
        }

        // Apply If-Flattening (redundant with block visit but handles some cases)
        if let ExpressionKind::If {
            condition: _,
            if_true,
            if_false: Some(if_false_id),
            ..
        } = &arena[*expr_id].kind
        {
            let if_true_id = *if_true;
            let if_false_id = *if_false_id;

            if self.is_terminator(if_true_id, arena) {
                if !self.is_terminator(if_false_id, arena) {
                    // This is only actionable if we're a child of a block,
                    // which is handled by the block visit above.
                }
            }
        }
    }
}

impl ControlFlowSimplifier {
    fn is_terminator(
        &self,
        id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        match &arena[id].kind {
            ExpressionKind::Return { .. } | ExpressionKind::Unreachable => true,
            ExpressionKind::Break { condition, .. } => condition.is_none(),
            ExpressionKind::Switch { .. } => true,
            ExpressionKind::Block { list, .. } => {
                if let Some(&last) = list.last() {
                    self.is_terminator(last, arena)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
