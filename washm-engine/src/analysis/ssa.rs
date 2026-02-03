use crate::analysis::cfg::{BlockId, ControlFlowGraph};
use crate::analysis::dominators::DominanceTree;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{Function, LocalId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefA {
    Instruction(ExpressionId),
    Phi(BlockId, LocalId),
    Param(LocalId),
}

#[derive(Debug)]
pub struct PhiNode {
    pub result_local: LocalId,
    pub incoming: HashMap<BlockId, DefA>,
}

pub struct SSABuilder {
    pub phi_nodes: HashMap<BlockId, Vec<PhiNode>>,
    // Map from original local to its SSA versions
    pub def_blocks: HashMap<LocalId, HashSet<BlockId>>,
    pub use_def: HashMap<ExpressionId, DefA>,
    pub def_uses: HashMap<DefA, Vec<ExpressionId>>,
}

impl Default for SSABuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SSABuilder {
    pub fn new() -> Self {
        Self {
            phi_nodes: HashMap::new(),
            def_blocks: HashMap::new(),
            use_def: HashMap::new(),
            def_uses: HashMap::new(),
        }
    }

    pub fn build(
        func: &Function,
        cfg: &ControlFlowGraph,
        dom: &DominanceTree,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Self {
        let mut builder = Self::new();
        builder.compute_phi_placements(func, cfg, dom, arena);
        builder.rename_variables(func, cfg, dom, arena);
        builder.build_def_uses();
        builder
    }

    fn build_def_uses(&mut self) {
        for (&use_expr, &def) in &self.use_def {
            self.def_uses.entry(def).or_default().push(use_expr);
        }
    }

    fn rename_variables(
        &mut self,
        func: &Function,
        cfg: &ControlFlowGraph,
        dom: &DominanceTree,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let mut stacks: HashMap<LocalId, Vec<DefA>> = HashMap::new();

        // 0. Initialize stacks with params
        for id in func.param_ids() {
            let local_id = *id;
            stacks
                .entry(local_id)
                .or_default()
                .push(DefA::Param(local_id));
        }

        self.rename_block(cfg.entry, cfg, dom, arena, &mut stacks);
    }

    fn rename_block(
        &mut self,
        block_id: BlockId,
        cfg: &ControlFlowGraph,
        dom: &DominanceTree,
        arena: &IdArena<ExpressionMarker, Expression>,
        stacks: &mut HashMap<LocalId, Vec<DefA>>,
    ) {
        let mut pushes: HashMap<LocalId, usize> = HashMap::new();

        // Phis
        if let Some(phis) = self.phi_nodes.get(&block_id) {
            for phi in phis {
                let def = DefA::Phi(block_id, phi.result_local);
                stacks.entry(phi.result_local).or_default().push(def);
                *pushes.entry(phi.result_local).or_default() += 1;
            }
        }

        // Instructions
        if let Some(block) = cfg.blocks.get(block_id) {
            for &expr_id in &block.contents {
                self.visit_expr_for_rename(expr_id, stacks, &mut pushes, arena);
            }

            // Fill in Phi operands for successors
            for succ_id in &block.succs {
                if let Some(phis) = self.phi_nodes.get_mut(succ_id) {
                    for phi in phis {
                        if let Some(stack) = stacks.get(&phi.result_local) {
                            if let Some(def) = stack.last() {
                                phi.incoming.insert(block_id, *def);
                            }
                        }
                    }
                }
            }

            // Recurse children
            // Naive search for children
            // In efficient impl, dom tree stores children list.
            for (child, &parent) in &dom.doms {
                if parent == block_id && *child != block_id {
                    self.rename_block(*child, cfg, dom, arena, stacks);
                }
            }
        }

        // Pop
        for (local, count) in pushes {
            let stack = stacks.get_mut(&local).unwrap();
            for _ in 0..count {
                stack.pop();
            }
        }
    }

    fn visit_expr_for_rename(
        &mut self,
        expr_id: ExpressionId,
        stacks: &mut HashMap<LocalId, Vec<DefA>>,
        pushes: &mut HashMap<LocalId, usize>,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[expr_id].kind {
            ExpressionKind::LocalGet { id } => {
                if let Some(stack) = stacks.get(id) {
                    if let Some(def) = stack.last() {
                        self.use_def.insert(expr_id, *def);
                    }
                }
            }
            ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                self.visit_expr_for_rename(*value, stacks, pushes, arena);
                let def = DefA::Instruction(expr_id);
                stacks.entry(*id).or_default().push(def);
                *pushes.entry(*id).or_default() += 1;
            }
            ExpressionKind::Block { list, .. } => {
                for &child in list {
                    self.visit_expr_for_rename(child, stacks, pushes, arena);
                }
            }
            ExpressionKind::If {
                name: _,
                condition,
                if_true,
                if_false,
            } => {
                self.visit_expr_for_rename(*condition, stacks, pushes, arena);
                self.visit_expr_for_rename(*if_true, stacks, pushes, arena);
                if let Some(fb) = if_false {
                    self.visit_expr_for_rename(*fb, stacks, pushes, arena);
                }
            }
            ExpressionKind::Unary { value, .. } => {
                self.visit_expr_for_rename(*value, stacks, pushes, arena)
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.visit_expr_for_rename(*left, stacks, pushes, arena);
                self.visit_expr_for_rename(*right, stacks, pushes, arena);
            }
            ExpressionKind::Call { operands, .. } => {
                for &op in operands {
                    self.visit_expr_for_rename(op, stacks, pushes, arena);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.visit_expr_for_rename(*body, stacks, pushes, arena)
            }
            ExpressionKind::Drop { value } => {
                self.visit_expr_for_rename(*value, stacks, pushes, arena)
            }
            _ => {}
        }
    }

    fn compute_phi_placements(
        &mut self,
        func: &Function,
        cfg: &ControlFlowGraph,
        dom: &DominanceTree,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        // 1. Compute join points for each local (where Phis are needed)
        // Set of blocks that define each local
        let mut defs: HashMap<LocalId, HashSet<BlockId>> = HashMap::new();

        // Entry block defines all params
        for &id in func.param_ids() {
            defs.entry(id).or_default().insert(cfg.entry);
        }

        // Scan CFG to find defs
        for (id, block) in cfg.blocks.iter() {
            for &expr_id in &block.contents {
                match &arena[expr_id].kind {
                    ExpressionKind::LocalSet { id: local_id, .. }
                    | ExpressionKind::LocalTee { id: local_id, .. } => {
                        defs.entry(*local_id).or_default().insert(id);
                    }
                    _ => {}
                }
            }
        }

        // 2. Insert Phi nodes at dominance frontiers
        for (local, blocks) in &defs {
            let mut worklist: Vec<BlockId> = blocks.iter().cloned().collect();
            let mut visited = HashSet::new();

            // Also add blocks already in visited to avoid reprocessing?
            // Actually `visited` tracks where we inserted Phi.
            // `has_already` tracks if we added block to worklist?
            // Standard algorithm uses `has_phi` and `has_def`.

            while let Some(b) = worklist.pop() {
                if let Some(frontier) = dom.frontiers.get(&b) {
                    for &f in frontier {
                        if !visited.contains(&f) {
                            visited.insert(f);

                            // Insert Phi for `local` at block `f`
                            self.phi_nodes.entry(f).or_default().push(PhiNode {
                                result_local: *local,
                                incoming: HashMap::new(),
                            });

                            // Phi node is a new def, so add to worklist
                            if !blocks.contains(&f) {
                                worklist.push(f);
                            }
                        }
                    }
                }
            }
        }

        self.def_blocks = defs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::cfg::ControlFlowGraph;
    use crate::analysis::dominators::DominanceTree;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Module};
    use crate::ir::{Literal, Type};

    #[test]
    fn test_ssa_phi_placement() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // (block
        //   (if (i32.const 1)
        //     (local.set 0 (i32.const 10))
        //     (local.set 0 (i32.const 20))
        //   )
        //   (local.get 0)
        // )

        // Join block (block 3 in CFG) should have Phi for local 0.

        let const1 = builder.const_(Literal::I32(1));
        let const10 = builder.const_(Literal::I32(10));
        let const20 = builder.const_(Literal::I32(20));

        let set0_1 = builder.local_set(0, const10);
        let set0_2 = builder.local_set(0, const20);

        let if_ = builder.if_(None::<&str>, const1, set0_1, Some(set0_2), Type::NONE);
        let get0 = builder.local_get(LocalId::from(0), Type::I32);

        let list = vec![if_, get0];

        let block = builder.block(None::<&str>, list, Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            block,
        );

        let cfg = ControlFlowGraph::build(&func, block, &module.expressions);
        let dom = DominanceTree::build(&cfg);

        let ssa = SSABuilder::build(&func, &cfg, &dom, &module.expressions);

        // Find the block containing get0
        let join_block_id = cfg.expr_to_block.get(&get0).unwrap();
        let phis = ssa.phi_nodes.get(join_block_id);
        assert!(phis.is_some());
        assert_eq!(phis.unwrap().len(), 1);
        assert_eq!(phis.unwrap()[0].result_local, 0.into());
    }

    #[test]
    fn test_ssa_renaming() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // (block
        //   (local.set 0 (i32.const 10))  <-- Def 1
        //   (local.get 0)                 <-- Use 1 (should point to Def 1)
        //   (local.set 0 (i32.const 20))  <-- Def 2
        //   (local.get 0)                 <-- Use 2 (should point to Def 2)
        // )

        let const10 = builder.const_(Literal::I32(10));
        let set1 = builder.local_set(0, const10);

        let get1 = builder.local_get(0, Type::I32);

        let const20 = builder.const_(Literal::I32(20));
        let set2 = builder.local_set(0, const20);

        let get2 = builder.local_get(0, Type::I32);

        let list = vec![set1, get1, set2, get2];

        let block = builder.block(None::<&str>, list, Type::NONE);
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            block,
        );

        let cfg = ControlFlowGraph::build(&func, block, &module.expressions);
        let dom = DominanceTree::build(&cfg);
        let ssa = SSABuilder::build(&func, &cfg, &dom, &module.expressions);

        // Check Use 1 -> Def 1
        if let Some(DefA::Instruction(def)) = ssa.use_def.get(&get1) {
            assert_eq!(*def, set1);
        } else {
            panic!("Use 1 not found or wrong type");
        }

        // Check Use 2 -> Def 2
        if let Some(DefA::Instruction(def)) = ssa.use_def.get(&get2) {
            assert_eq!(*def, set2);
        } else {
            panic!("Use 2 not found or wrong type");
        }
    }
}
