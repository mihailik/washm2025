use crate::collections::id_arena::{Id, IdArena};
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Function;
use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct BlockMarker;
pub type BlockId = Id<BlockMarker>;

#[derive(Debug, Clone)]
pub struct BasicBlockA<'a> {
    pub preds: Vec<BlockId>,
    pub succs: Vec<BlockId>,
    pub contents: Vec<ExprRef<'a>>,
    pub is_virtual: bool,
}

pub struct ControlFlowGraph<'a> {
    pub blocks: IdArena<BlockMarker, BasicBlockA<'a>>,
    pub entry: BlockId,
    pub exit: BlockId,
    pub expr_to_block: HashMap<ExprRef<'a>, BlockId>,
}

impl<'a> ControlFlowGraph<'a> {
    pub fn build(_func: &Function, entry_expr: ExprRef<'a>) -> Self {
        let mut builder = CFGBuilder::new();
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
    cfg: ControlFlowGraph<'a>,
    current_block: BlockId,
    labels: HashMap<&'a str, BlockId>,
}

impl<'a> CFGBuilder<'a> {
    fn new() -> Self {
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
            labels: HashMap::new(),
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

    fn build(&mut self, expr: ExprRef<'a>) {
        // Simple recursive traversal that splits blocks at control flow
        self.visit(expr);
    }

    fn visit(&mut self, expr: ExprRef<'a>) {
        // Map expression to current block
        self.cfg.expr_to_block.insert(expr, self.current_block);

        // Add to current block
        self.cfg.blocks[self.current_block].contents.push(expr);

        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let join_block = self.create_block();

                // Save old label if shadowed
                let old_label = name.and_then(|n| self.labels.insert(n, join_block));

                for child in list.iter() {
                    self.visit(*child);
                }

                // End of block falls through to join
                self.add_edge(self.current_block, join_block);

                // Restore label
                if let Some(n) = name {
                    if let Some(old) = old_label {
                        self.labels.insert(n, old);
                    } else {
                        self.labels.remove(n);
                    }
                }

                self.current_block = join_block;
            }
            ExpressionKind::If {
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
            ExpressionKind::Loop { name, body } => {
                let entry_block = self.current_block;
                let loop_header = self.create_block();
                let loop_exit = self.create_block();

                self.add_edge(entry_block, loop_header);

                // For a loop, the label points to the header
                let old_label = name.and_then(|n| self.labels.insert(n, loop_header));

                self.current_block = loop_header;
                self.visit(*body);

                // Structural fallthrough to exit
                self.add_edge(self.current_block, loop_exit);

                if let Some(n) = name {
                    if let Some(old) = old_label {
                        self.labels.insert(n, old);
                    } else {
                        self.labels.remove(n);
                    }
                }

                self.current_block = loop_exit;
            }
            ExpressionKind::Break {
                name, condition, ..
            } => {
                if let Some(cond) = condition {
                    self.visit(*cond);
                }

                let entry_block = self.current_block;
                if let Some(&target) = self.labels.get(name) {
                    self.add_edge(entry_block, target);
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
                names,
                default,
                condition,
                value,
            } => {
                self.visit(*condition);
                if let Some(v) = value {
                    self.visit(*v);
                }

                let entry_block = self.current_block;
                for &name in names.iter().chain(std::iter::once(default)) {
                    if let Some(&target) = self.labels.get(name) {
                        self.add_edge(entry_block, target);
                    }
                }

                let next_block = self.create_block();
                self.current_block = next_block;
            }
            _ => match &expr.kind {
                ExpressionKind::Binary { left, right, .. } => {
                    self.visit(*left);
                    self.visit(*right);
                }
                ExpressionKind::Unary { value, .. } => {
                    self.visit(*value);
                }
                ExpressionKind::Call { operands, .. } => {
                    for op in operands.iter() {
                        self.visit(*op);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    self.visit(*target);
                    for op in operands.iter() {
                        self.visit(*op);
                    }
                }
                ExpressionKind::LocalSet { value, .. } => self.visit(*value),
                ExpressionKind::LocalTee { value, .. } => self.visit(*value),
                ExpressionKind::GlobalSet { value, .. } => self.visit(*value),
                ExpressionKind::Drop { value } => self.visit(*value),
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit(*condition);
                    self.visit(*if_true);
                    self.visit(*if_false);
                }
                ExpressionKind::Load { ptr, .. } => self.visit(*ptr),
                ExpressionKind::Store { ptr, value, .. } => {
                    self.visit(*ptr);
                    self.visit(*value);
                }
                ExpressionKind::MemoryGrow { delta, .. } => self.visit(*delta),
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    self.visit(*ptr);
                    self.visit(*value);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    self.visit(*ptr);
                    self.visit(*expected);
                    self.visit(*replacement);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    self.visit(*ptr);
                    self.visit(*expected);
                    self.visit(*timeout);
                }
                ExpressionKind::AtomicNotify { ptr, count, .. } => {
                    self.visit(*ptr);
                    self.visit(*count);
                }
                ExpressionKind::SIMDExtract { vec, .. } => self.visit(*vec),
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    self.visit(*vec);
                    self.visit(*value);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    self.visit(*left);
                    self.visit(*right);
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_cfg_construction() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (block
        //   (if (i32.const 1)
        //     (nop)
        //     (nop)
        //   )
        // )

        let const1 = builder.const_(Literal::I32(1));
        let nop1 = builder.nop();
        let nop2 = builder.nop();
        let if_ = builder.if_(const1, nop1, Some(nop2), Type::NONE);

        let mut list = BumpVec::new_in(&bump);
        list.push(if_);
        let block = builder.block(None, list, Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );

        let cfg = ControlFlowGraph::build(&func, block);

        // Expected blocks:
        // Entry
        // Virtual Exit
        // Block Join
        // If Then
        // If Else
        // If Join

        assert_eq!(cfg.blocks.len(), 6);

        // Check edges
        let entry_succs = &cfg.blocks[cfg.entry].succs;
        assert_eq!(entry_succs.len(), 2);

        let then_id = entry_succs[0];
        let else_id = entry_succs[1];

        // Join should be a common successor if we look deep enough,
        // but let's just check that they have successors.
        assert!(!cfg.blocks[then_id].succs.is_empty());
        assert!(!cfg.blocks[else_id].succs.is_empty());
    }

    #[test]
    fn test_cfg_loop() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (loop
        //   (nop)
        // )

        let nop = builder.nop();
        let loop_ = builder.loop_(Some("loop"), nop, Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(loop_),
        );

        let cfg = ControlFlowGraph::build(&func, loop_);

        // Blocks:
        // Entry
        // Virtual Exit
        // Header (Loop)
        // Exit (Loop)

        assert_eq!(cfg.blocks.len(), 4);
        let entry_succs = &cfg.blocks[cfg.entry].succs;
        assert_eq!(entry_succs.len(), 1);
        let header_id = entry_succs[0];
        assert!(!cfg.blocks[header_id].succs.is_empty());
    }
}
