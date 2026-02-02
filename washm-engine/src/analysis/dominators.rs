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
        let all_nodes: HashSet<BlockId> = blocks.iter().map(|b| b.id).collect();
        let mut dom_sets: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        dom_sets.insert(cfg.entry, [cfg.entry].iter().cloned().collect());
        for b in blocks {
            if b.id != cfg.entry {
                dom_sets.insert(b.id, all_nodes.clone());
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for b in blocks {
                if b.id == cfg.entry {
                    continue;
                }

                // dom(n) = {n} U (intersection of dom(p) for all preds p)
                let mut new_dom = all_nodes.clone(); // Start with all

                if b.preds.is_empty() {
                    // Unreachable from entry?
                    // Should be handled, but intersect of empty is empty (or universe?)
                    // In data flow, intersection over empty set is usually universe (Top).
                    // But if it has no preds and is not entry, it's unreachable.
                    // We skip unreachable blocks in dominance calculation usually.
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

                new_dom.insert(b.id);

                if dom_sets.get(&b.id) != Some(&new_dom) {
                    dom_sets.insert(b.id, new_dom);
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

        for b in blocks {
            if b.preds.len() >= 2 {
                for &p in &b.preds {
                    let mut runner = p;
                    while runner != *doms.get(&b.id).unwrap_or(&cfg.entry) {
                        // Approx check
                        // Add b to frontier of runner
                        frontiers
                            .entry(runner)
                            .or_insert_with(HashSet::new)
                            .insert(b.id);

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

        let all_nodes: HashSet<BlockId> = blocks.iter().map(|b| b.id).collect();
        let mut pdom_sets: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        pdom_sets.insert(exit, [exit].iter().cloned().collect());
        for b in blocks {
            if b.id != exit {
                pdom_sets.insert(b.id, all_nodes.clone());
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for b in blocks {
                if b.id == exit {
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

                new_pdom.insert(b.id);

                if pdom_sets.get(&b.id) != Some(&new_pdom) {
                    pdom_sets.insert(b.id, new_pdom);
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
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_dominance_diamond() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Diamond shape CFG (like if-then-else)
        // 0 -> 1, 2
        // 1 -> 3
        // 2 -> 3

        let const1 = builder.const_(Literal::I32(1));
        let nop1 = builder.nop();
        let nop2 = builder.nop();
        let if_ = builder.if_(const1, nop1, Some(nop2), Type::NONE);

        let mut list = bumpalo::collections::Vec::new_in(&bump);
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
        let dom = DominanceTree::build(&cfg);

        // Blocks:
        // 0: Entry
        // 1: Virtual Exit
        // 2: Block Join
        // 3: If Then
        // 4: If Else
        // 5: If Join

        assert_eq!(*dom.doms.get(&3).unwrap(), 0);
        assert_eq!(*dom.doms.get(&4).unwrap(), 0);
        assert_eq!(*dom.doms.get(&5).unwrap(), 0);
        assert_eq!(*dom.doms.get(&2).unwrap(), 5);

        // Frontiers:
        // DF(3) = {5} (3 dominates predecessor of 5, but not 5)
        // DF(4) = {5}

        let empty = HashSet::new();
        let df3 = dom.frontiers.get(&3).unwrap_or(&empty);
        assert!(df3.contains(&5));

        let df4 = dom.frontiers.get(&4).unwrap_or(&empty);
        assert!(df4.contains(&5));
    }

    #[test]
    fn test_dominance_nested() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Nested IFs
        // (if (c1)
        //   (if (c2)
        //     (nop)
        //     (nop)
        //   )
        //   (nop)
        // )

        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let nop = builder.nop();

        let inner_if = builder.if_(c2, nop, Some(nop), Type::NONE);
        let outer_if = builder.if_(c1, inner_if, Some(nop), Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(outer_if),
        );

        let cfg = ControlFlowGraph::build(&func, outer_if);
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

        // Structure:
        // 0 -> 2, 3
        // 2 -> 5, 6
        // 5 -> 7
        // 6 -> 7
        // 7 -> 4
        // 3 -> 4
        // 4 -> 1

        // Check immediate dominators:
        // idom(2) = 0
        assert_eq!(*dom.doms.get(&2).unwrap(), 0);
        // idom(3) = 0
        assert_eq!(*dom.doms.get(&3).unwrap(), 0);
        // idom(4) = 0
        assert_eq!(*dom.doms.get(&4).unwrap(), 0);

        // idom(5) = 2
        assert_eq!(*dom.doms.get(&5).unwrap(), 2);
        // idom(6) = 2
        assert_eq!(*dom.doms.get(&6).unwrap(), 2);
        // idom(7) = 2
        assert_eq!(*dom.doms.get(&7).unwrap(), 2);
    }
}
