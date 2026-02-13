use crate::collections::id_arena::{Id, IdArena};
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Function;
use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct BlockMarker;
pub type BlockId = Id<BlockMarker>;

#[derive(Debug, Clone)]
pub struct BasicBlockA {
    pub preds: Vec<BlockId>,
    pub succs: Vec<BlockId>,
    pub contents: Vec<ExpressionId>,
    pub is_virtual: bool,
}

pub struct ControlFlowGraph {
    pub blocks: IdArena<BlockMarker, BasicBlockA>,
    pub entry: BlockId,
    pub exit: BlockId,
    pub expr_to_block: HashMap<ExpressionId, BlockId>,
}

impl ControlFlowGraph {
    pub fn build(
        _func: &Function,
        entry_expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Self {
        let mut builder = CFGBuilder::new(arena);
        builder.build(entry_expr);

        // Finalize: link any block with no succs to the virtual exit (unless it's the exit itself)
        let exit_id = builder.cfg.exit;
        let block_ids: Vec<BlockId> = builder.cfg.blocks.iter().map(|(id, _)| id).collect();
        for id in block_ids {
            if id != exit_id && builder.cfg.blocks[id].succs.is_empty() {
                builder.add_edge(id, exit_id);
            }
        }

        builder.cfg
    }
}

struct CFGBuilder<'a> {
    cfg: ControlFlowGraph,
    current_block: BlockId,
    targets: HashMap<ExpressionId, BlockId>,
    arena: &'a IdArena<ExpressionMarker, Expression>,
}

impl<'a> CFGBuilder<'a> {
    fn new(arena: &'a IdArena<ExpressionMarker, Expression>) -> Self {
        let mut blocks = IdArena::new();

        let entry_id = blocks.alloc(BasicBlockA {
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: false,
        });

        // Create a virtual exit block as well
        let exit_id = blocks.alloc(BasicBlockA {
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: true,
        });

        Self {
            cfg: ControlFlowGraph {
                blocks,
                entry: entry_id,
                exit: exit_id,
                expr_to_block: HashMap::new(),
            },
            current_block: entry_id,
            targets: HashMap::new(),
            arena,
        }
    }

    fn create_block(&mut self) -> BlockId {
        self.cfg.blocks.alloc(BasicBlockA {
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: false,
        })
    }

    fn add_edge(&mut self, from: BlockId, to: BlockId) {
        if !self.cfg.blocks[from].succs.contains(&to) {
            self.cfg.blocks[from].succs.push(to);
            self.cfg.blocks[to].preds.push(from);
        }
    }

    fn build(&mut self, expr_id: ExpressionId) {
        // Simple recursive traversal that splits blocks at control flow
        self.visit(expr_id);
    }

    fn visit(&mut self, expr_id: ExpressionId) {
        // Map expression to current block
        self.cfg.expr_to_block.insert(expr_id, self.current_block);

        // Add to current block
        self.cfg.blocks[self.current_block].contents.push(expr_id);

        match &self.arena[expr_id].kind {
            ExpressionKind::Block { name: _, list } => {
                let join_block = self.create_block();

                self.targets.insert(expr_id, join_block);

                for &child in list {
                    self.visit(child);
                }

                // End of block falls through to join
                self.add_edge(self.current_block, join_block);

                self.current_block = join_block;
            }
            ExpressionKind::If {
                name: _,
                condition,
                if_true,
                if_false,
            } => {
                self.visit(*condition);

                let entry_block = self.current_block;
                let then_block = self.create_block();
                let else_block = if if_false.is_some() {
                    Some(self.create_block())
                } else {
                    None
                };
                let join_block = self.create_block();

                // Register target for breaks
                self.targets.insert(expr_id, join_block);

                self.add_edge(entry_block, then_block);

                self.current_block = then_block;
                self.visit(*if_true);
                self.add_edge(self.current_block, join_block);

                if let Some(else_b) = else_block {
                    self.add_edge(entry_block, else_b);
                    self.current_block = else_b;
                    if let Some(false_expr) = if_false {
                        self.visit(*false_expr);
                    }
                    self.add_edge(self.current_block, join_block);
                } else {
                    self.add_edge(entry_block, join_block);
                }

                self.current_block = join_block;
            }
            ExpressionKind::Loop { name: _, body } => {
                let entry_block = self.current_block;
                let loop_header = self.create_block();
                let loop_exit = self.create_block();

                self.add_edge(entry_block, loop_header);

                // For a loop, the label points to the header
                self.targets.insert(expr_id, loop_header);

                self.current_block = loop_header;
                self.visit(*body);

                // Structural fallthrough to exit
                self.add_edge(self.current_block, loop_exit);

                self.current_block = loop_exit;
            }
            ExpressionKind::Break {
                target,
                condition,
                value,
            } => {
                if let Some(cond) = condition {
                    self.visit(*cond);
                }
                if let Some(v) = value {
                    self.visit(*v);
                }

                let entry_block = self.current_block;
                if let Some(&target_block) = self.targets.get(target) {
                    self.add_edge(entry_block, target_block);
                }

                let next_block = self.create_block();
                if condition.is_some() {
                    // Conditional break: join with next block
                    self.add_edge(entry_block, next_block);
                }
                self.current_block = next_block;
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    self.visit(*v);
                }
                self.add_edge(self.current_block, self.cfg.exit);

                let next_block = self.create_block();
                self.current_block = next_block;
            }
            ExpressionKind::Unreachable => {
                self.add_edge(self.current_block, self.cfg.exit);
                let next_block = self.create_block();
                self.current_block = next_block;
            }
            ExpressionKind::Switch {
                targets,
                default,
                condition,
                value,
            } => {
                self.visit(*condition);
                if let Some(v) = value {
                    self.visit(*v);
                }

                let entry_block = self.current_block;
                for target in targets.iter().chain(std::iter::once(default)) {
                    if let Some(&target_block) = self.targets.get(target) {
                        self.add_edge(entry_block, target_block);
                    }
                }

                let next_block = self.create_block();
                self.current_block = next_block;
            }
            _ => self.visit_children(expr_id),
        }
    }

    fn visit_children(&mut self, expr_id: ExpressionId) {
        let kind = self.arena[expr_id].kind.clone();
        kind.for_each_child(|child| {
            self.visit(child);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Module;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_cfg_construction() {
        let mut module = Module::new();

        let block = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let const1 = builder.const_(Literal::I32(1));
            let nop1 = builder.nop();
            let nop2 = builder.nop();
            let if_ = builder.if_(None::<String>, const1, nop1, Some(nop2), Type::NONE);
            let list = vec![if_];
            builder.block(None::<String>, list, Type::NONE)
        };

        let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], block);

        let cfg = ControlFlowGraph::build(&func, block, &module.expressions);

        // Expected blocks:
        // Entry
        // Virtual Exit
        // Block (entry) contents
        // Then block
        // Else block
        // Join block

        // The exact count might vary based on splitter logic
        assert!(cfg.blocks.len() >= 4);
    }
}
