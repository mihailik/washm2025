use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Function;

pub type BlockId = u32;

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub preds: Vec<BlockId>,
    pub succs: Vec<BlockId>,
    pub contents: Vec<ExprRef<'static>>, // Using static for now as placeholder for 'a, fixed in impl
}

// We need a way to refer to ExprRef with lifetime 'a in the struct.
// But CFG is usually short-lived or tied to module lifetime.
// Let's use 'a.

#[derive(Debug)]
pub struct BasicBlockA<'a> {
    pub id: BlockId,
    pub preds: Vec<BlockId>,
    pub succs: Vec<BlockId>,
    pub contents: Vec<ExprRef<'a>>,
    pub is_virtual: bool,
}

pub struct ControlFlowGraph<'a> {
    pub blocks: Vec<BasicBlockA<'a>>,
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
        for i in 0..builder.cfg.blocks.len() {
            if i as u32 != exit_id && builder.cfg.blocks[i].succs.is_empty() {
                let from = i as u32;
                builder.add_edge(from, exit_id);
            }
        }

        builder.cfg
    }
}

use std::collections::HashMap;

struct CFGBuilder<'a> {
    cfg: ControlFlowGraph<'a>,
    current_block: BlockId,
    labels: HashMap<&'a str, BlockId>,
}

impl<'a> CFGBuilder<'a> {
    fn new() -> Self {
        let entry_block = BasicBlockA {
            id: 0,
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: false,
        };

        // Create a virtual exit block as well
        let exit_block = BasicBlockA {
            id: 1,
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: true,
        };

        Self {
            cfg: ControlFlowGraph {
                blocks: vec![entry_block, exit_block],
                entry: 0,
                exit: 1,
                expr_to_block: HashMap::new(),
            },
            current_block: 0,
            labels: HashMap::new(),
        }
    }

    fn create_block(&mut self) -> BlockId {
        let id = self.cfg.blocks.len() as u32;
        self.cfg.blocks.push(BasicBlockA {
            id,
            preds: vec![],
            succs: vec![],
            contents: vec![],
            is_virtual: false,
        });
        id
    }

    fn add_edge(&mut self, from: BlockId, to: BlockId) {
        if !self.cfg.blocks[from as usize].succs.contains(&to) {
            self.cfg.blocks[from as usize].succs.push(to);
            self.cfg.blocks[to as usize].preds.push(from);
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
        self.cfg.blocks[self.current_block as usize]
            .contents
            .push(expr);

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
                ExpressionKind::MemoryGrow { delta } => self.visit(*delta),
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
                ExpressionKind::AtomicNotify { ptr, count } => {
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
        // 0: Entry
        // 1: Virtual Exit
        // 2: Block Join
        // 3: If Then
        // 4: If Else
        // 5: If Join

        assert_eq!(cfg.blocks.len(), 6);

        // Check edges
        // Entry (0) should have edges to Then (3) and Else (4) via If visit
        assert!(cfg.blocks[0].succs.contains(&3));
        assert!(cfg.blocks[0].succs.contains(&4));

        // 3 -> 5
        assert!(cfg.blocks[3].succs.contains(&5));

        // 4 -> 5
        assert!(cfg.blocks[4].succs.contains(&5));
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

        // Expected:
        // 0: Entry -> 1 (Loop Header)
        // 1: Header -> 2 (Exit)
        // 2: Exit

        // Note: My implementation above doesn't add back edge automatically for Loop expr.
        // It relies on explicit 'br' inside the loop to jump back.
        // So this loop just falls through.

        // Blocks:
        // 0: Entry
        // 1: Virtual Exit
        // 2: Header (Loop)
        // 3: Exit (Loop)

        assert_eq!(cfg.blocks.len(), 4);
        assert!(cfg.blocks[0].succs.contains(&2));
        assert!(cfg.blocks[2].succs.contains(&3));
    }
}
