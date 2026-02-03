use crate::analysis::cfg::{BlockId, ControlFlowGraph};
use std::collections::{HashMap, HashSet};

pub struct DominanceTree {
    pub doms: HashMap<BlockId, BlockId>, // id -> immediate dominator
    pub frontiers: HashMap<BlockId, HashSet<BlockId>>,
}

impl DominanceTree {
    pub fn build(cfg: &ControlFlowGraph) -> Self {
        let mut doms = HashMap::new();
        let blocks = &cfg.blocks;
        let num_blocks = blocks.len();

        if num_blocks == 0 {
            return Self {
                doms,
                frontiers: HashMap::new(),
            };
        }

        // Initialize dominators
        // Entry dominates itself
        doms.insert(cfg.entry, cfg.entry);

        // Others initialized to all blocks (represented as None or specific check)
        // Here we use iterative algorithm

        // Initial state: dom(entry) = {entry}, others = all
        let all_nodes: HashSet<BlockId> = blocks.iter().map(|(id, _)| id).collect();
        let mut dom_sets: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        dom_sets.insert(cfg.entry, [cfg.entry].iter().cloned().collect());
        for (id, _) in blocks.iter() {
            if id != cfg.entry {
                dom_sets.insert(id, all_nodes.clone());
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for (id, b) in blocks.iter() {
                if id == cfg.entry {
                    continue;
                }

                // dom(n) = {n} U (intersection of dom(p) for all preds p)
                let mut new_dom = all_nodes.clone(); // Start with all

                if b.preds.is_empty() {
                    continue;
                }

                let mut first = true;
                for &pred in &b.preds {
                    if let Some(pred_doms) = dom_sets.get(&pred) {
                        if first {
                            new_dom = pred_doms.clone();
                            first = false;
                        } else {
                            new_dom.retain(|x| pred_doms.contains(x));
                        }
                    }
                }

                new_dom.insert(id);

                if dom_sets.get(&id) != Some(&new_dom) {
                    dom_sets.insert(id, new_dom);
                    changed = true;
                }
            }
        }

        // Compute immediate dominators
        for (&n, dominators) in &dom_sets {
            // idom(n) is the unique node d in dominators such that d != n and d dominates every other node in dominators\{n}
            if n == cfg.entry {
                continue;
            }

            let candidates: Vec<BlockId> = dominators.iter().cloned().filter(|&d| d != n).collect();
            // Find the one that is dominated by all others (closest)
            // Actually, idom dominates n, and is dominated by all strict dominators of n.
            // So idom is the "largest" strict dominator.
            // A dominates B if A is in dom_sets[B].

            if let Some(&idom) = candidates.iter().find(|&&candidate| {
                // Check if candidate is dominated by all other candidates
                candidates.iter().all(|&other| {
                    if other == candidate {
                        true
                    } else {
                        // other dominates candidate?
                        dom_sets.get(&candidate).unwrap().contains(&other)
                    }
                })
            }) {
                doms.insert(n, idom);
            }
        }

        // Compute dominance frontiers
        // DF(n) = { y | n dom pred(y) but not n sdom y }
        let mut frontiers = HashMap::new();

        for (id, b) in blocks.iter() {
            if b.preds.len() >= 2 {
                for &p in &b.preds {
                    let mut runner = p;
                    while runner != *doms.get(&id).unwrap_or(&cfg.entry) {
                        // Approx check
                        // Add b to frontier of runner
                        frontiers
                            .entry(runner)
                            .or_insert_with(HashSet::new)
                            .insert(id);

                        if let Some(&d) = doms.get(&runner) {
                            if d == runner {
                                break;
                            } // Root
                            runner = d;
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        Self { doms, frontiers }
    }
}

pub struct PostDominanceTree {
    pub pdoms: HashMap<BlockId, BlockId>, // id -> immediate post-dominator
}

impl PostDominanceTree {
    pub fn build(cfg: &ControlFlowGraph) -> Self {
        let mut pdoms = HashMap::new();
        let blocks = &cfg.blocks;
        let num_blocks = blocks.len();

        if num_blocks == 0 {
            return Self { pdoms };
        }

        // Start node for post-dominance is the exit node
        let exit = cfg.exit;
        pdoms.insert(exit, exit);

        let all_nodes: HashSet<BlockId> = blocks.iter().map(|(id, _)| id).collect();
        let mut pdom_sets: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        pdom_sets.insert(exit, [exit].iter().cloned().collect());
        for (id, _) in blocks.iter() {
            if id != exit {
                pdom_sets.insert(id, all_nodes.clone());
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for (id, b) in blocks.iter() {
                if id == exit {
                    continue;
                }

                // pdom(n) = {n} U (intersection of pdom(s) for all succs s)
                let mut new_pdom = all_nodes.clone();

                if b.succs.is_empty() {
                    // Block with no successors (e.g. infinite loop if not connected to exit)
                    // If it's not the exit node, it might not reach exit.
                    continue;
                }

                let mut first = true;
                for &succ in &b.succs {
                    if let Some(succ_pdoms) = pdom_sets.get(&succ) {
                        if first {
                            new_pdom = succ_pdoms.clone();
                            first = false;
                        } else {
                            new_pdom.retain(|x| succ_pdoms.contains(x));
                        }
                    }
                }

                new_pdom.insert(id);

                if pdom_sets.get(&id) != Some(&new_pdom) {
                    pdom_sets.insert(id, new_pdom);
                    changed = true;
                }
            }
        }

        // Compute immediate post-dominators
        for (&n, post_dominators) in &pdom_sets {
            if n == exit {
                continue;
            }

            let candidates: Vec<BlockId> = post_dominators
                .iter()
                .cloned()
                .filter(|&d| d != n)
                .collect();

            // ipdom(n) is the strict post-dominator that is post-dominated by all other strict post-dominators
            if let Some(&ipdom) = candidates.iter().find(|&&candidate| {
                candidates.iter().all(|&other| {
                    if other == candidate {
                        true
                    } else {
                        // other post-dominates candidate?
                        pdom_sets.get(&candidate).unwrap().contains(&other)
                    }
                })
            }) {
                pdoms.insert(n, ipdom);
            }
        }

        Self { pdoms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::id_arena::IdArena;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_dominance_diamond() {
        let mut arena = IdArena::new();

        let (block, func) = {
            let mut builder = IrBuilder::new(&mut arena);
            let const1 = builder.const_(Literal::I32(1));
            let nop1 = builder.nop();
            let nop2 = builder.nop();
            let if_ = builder.if_(None::<String>, const1, nop1, Some(nop2), Type::NONE);
            let list = vec![if_];
            let block = builder.block(None::<String>, list, Type::NONE);
            let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], block);
            (block, func)
        };

        let cfg = ControlFlowGraph::build(&func, block, &arena);
        let dom = DominanceTree::build(&cfg);

        // Blocks:
        // 0: Entry
        // 1: Virtual Exit
        // 2: Block Join
        // 3: If Then
        // 4: If Else
        // 5: If Join

        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(3)).unwrap(),
            BlockId::from_u32(0)
        );
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(4)).unwrap(),
            BlockId::from_u32(0)
        );
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(5)).unwrap(),
            BlockId::from_u32(0)
        );
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(2)).unwrap(),
            BlockId::from_u32(5)
        );

        // Frontiers:
        // DF(3) = {5} (3 dominates predecessor of 5, but not 5)
        // DF(4) = {5}

        let empty = HashSet::new();
        let df3 = dom.frontiers.get(&BlockId::from_u32(3)).unwrap_or(&empty);
        assert!(df3.contains(&BlockId::from_u32(5)));

        let df4 = dom.frontiers.get(&BlockId::from_u32(4)).unwrap_or(&empty);
        assert!(df4.contains(&BlockId::from_u32(5)));
    }

    #[test]
    fn test_dominance_nested() {
        let mut arena = IdArena::new();

        let (outer_if, func) = {
            let mut builder = IrBuilder::new(&mut arena);
            let c1 = builder.const_(Literal::I32(1));
            let c2 = builder.const_(Literal::I32(2));
            let nop = builder.nop();

            let inner_if = builder.if_(None::<String>, c2, nop, Some(nop), Type::NONE);
            let outer_if = builder.if_(None::<String>, c1, inner_if, Some(nop), Type::NONE);

            let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], outer_if);
            (outer_if, func)
        };

        let cfg = ControlFlowGraph::build(&func, outer_if, &arena);
        let dom = DominanceTree::build(&cfg);

        // Expected block IDs based on traversal order:
        // 0: Entry
        // 1: Exit
        // 2: Outer Then
        // 3: Outer Else
        // 4: Outer Join
        // 5: Inner Then
        // 6: Inner Else
        // 7: Inner Join

        // Check immediate dominators:
        // idom(2) = 0
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(2)).unwrap(),
            BlockId::from_u32(0)
        );
        // idom(3) = 0
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(3)).unwrap(),
            BlockId::from_u32(0)
        );
        // idom(4) = 0
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(4)).unwrap(),
            BlockId::from_u32(0)
        );

        // idom(5) = 2
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(5)).unwrap(),
            BlockId::from_u32(2)
        );
        // idom(6) = 2
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(6)).unwrap(),
            BlockId::from_u32(2)
        );
        // idom(7) = 2
        assert_eq!(
            *dom.doms.get(&BlockId::from_u32(7)).unwrap(),
            BlockId::from_u32(2)
        );
    }
}
