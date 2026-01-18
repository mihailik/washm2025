use crate::analysis::dataflow::cfg::ControlFlowGraph;
use std::collections::{HashMap, HashSet};

/// Dominance tree for control flow analysis
///
/// A node A dominates node B if all paths from entry to B pass through A.
/// The immediate dominator (idom) is the unique closest dominator.
#[derive(Debug, Clone)]
pub struct DominanceTree {
    /// Maps block index to its immediate dominator
    idom: HashMap<usize, usize>,

    /// Maps block index to set of blocks it dominates
    dominates: HashMap<usize, HashSet<usize>>,

    /// Entry block (dominates all)
    entry: usize,
}

impl DominanceTree {
    /// Build dominance tree from control flow graph
    pub fn build(cfg: &ControlFlowGraph) -> Self {
        let entry = cfg.entry;
        let mut tree = DominanceTree {
            idom: HashMap::new(),
            dominates: HashMap::new(),
            entry,
        };

        if cfg.blocks.is_empty() {
            return tree;
        }

        // Calculate immediate dominators using iterative algorithm
        tree.calculate_idom(cfg);

        // Build dominates sets from idom
        tree.build_dominates_sets(cfg);

        tree
    }

    /// Calculate immediate dominators using Lengauer-Tarjan-style algorithm
    fn calculate_idom(&mut self, cfg: &ControlFlowGraph) {
        let n = cfg.blocks.len();
        let entry = self.entry;

        // Initialize: entry dominates itself
        self.idom.insert(entry, entry);

        // Iterative dataflow: compute dominators
        let mut changed = true;
        while changed {
            changed = false;

            for i in 0..n {
                if i == entry {
                    continue;
                }

                // New dominator is intersection of predecessors' dominators
                let preds = &cfg.blocks[i].preds;
                if preds.is_empty() {
                    continue;
                }

                // Start with first processed predecessor
                let mut new_idom = None;
                for &pred in preds {
                    if self.idom.contains_key(&pred) {
                        new_idom = Some(pred);
                        break;
                    }
                }

                if let Some(mut current_idom) = new_idom {
                    // Intersect with other predecessors
                    for &pred in preds {
                        if !self.idom.contains_key(&pred) {
                            continue;
                        }
                        current_idom = self.intersect(current_idom, pred);
                    }

                    // Update if changed
                    if self.idom.get(&i) != Some(&current_idom) {
                        self.idom.insert(i, current_idom);
                        changed = true;
                    }
                }
            }
        }
    }

    /// Find intersection of two dominator paths (lowest common ancestor)
    fn intersect(&self, mut b1: usize, mut b2: usize) -> usize {
        while b1 != b2 {
            while b1 > b2 {
                if let Some(&idom) = self.idom.get(&b1) {
                    if idom == b1 {
                        break; // Entry node
                    }
                    b1 = idom;
                } else {
                    break;
                }
            }
            while b2 > b1 {
                if let Some(&idom) = self.idom.get(&b2) {
                    if idom == b2 {
                        break; // Entry node
                    }
                    b2 = idom;
                } else {
                    break;
                }
            }
        }
        b1
    }

    /// Build dominates sets from immediate dominator information
    fn build_dominates_sets(&mut self, cfg: &ControlFlowGraph) {
        // Initialize empty sets
        for i in 0..cfg.blocks.len() {
            self.dominates.insert(i, HashSet::new());
        }

        // Every block dominates itself
        for i in 0..cfg.blocks.len() {
            self.dominates.get_mut(&i).unwrap().insert(i);
        }

        // Build dominance frontier by walking idom tree
        for (node, &idom) in &self.idom {
            if *node != idom {
                // Walk up dominator tree and add node to all ancestors' dominates sets
                let mut current = idom;
                loop {
                    self.dominates.get_mut(&current).unwrap().insert(*node);
                    if let Some(&next_idom) = self.idom.get(&current) {
                        if next_idom == current {
                            break; // Reached entry
                        }
                        current = next_idom;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    /// Check if dominator dominates dominated
    pub fn dominates(&self, dominator: usize, dominated: usize) -> bool {
        self.dominates
            .get(&dominator)
            .map(|set| set.contains(&dominated))
            .unwrap_or(false)
    }

    /// Get immediate dominator of a block
    pub fn idom(&self, block: usize) -> Option<usize> {
        self.idom.get(&block).copied()
    }

    /// Get all blocks dominated by a given block
    pub fn dominated_by(&self, block: usize) -> HashSet<usize> {
        self.dominates.get(&block).cloned().unwrap_or_default()
    }

    /// Find lowest common ancestor (LCA) in dominator tree
    pub fn lca(&self, a: usize, b: usize) -> Option<usize> {
        // Build path from a to entry
        let mut path_a = HashSet::new();
        let mut current = a;
        loop {
            path_a.insert(current);
            if let Some(&idom) = self.idom.get(&current) {
                if idom == current {
                    break; // Entry
                }
                current = idom;
            } else {
                break;
            }
        }

        // Walk from b to entry, find first node in path_a
        current = b;
        loop {
            if path_a.contains(&current) {
                return Some(current);
            }
            if let Some(&idom) = self.idom.get(&current) {
                if idom == current {
                    break; // Entry
                }
                current = idom;
            } else {
                break;
            }
        }

        None
    }

    /// Check if block is entry
    pub fn is_entry(&self, block: usize) -> bool {
        block == self.entry
    }

    /// Get entry block
    pub fn entry(&self) -> usize {
        self.entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::dataflow::cfg::ControlFlowGraph;

    #[test]
    fn test_dominance_simple_sequence() {
        // Linear CFG: 0 -> 1 -> 2
        let mut cfg = ControlFlowGraph::new();
        cfg.add_block(); // 0
        cfg.add_block(); // 1
        cfg.add_block(); // 2
        cfg.add_edge(0, 1);
        cfg.add_edge(1, 2);

        let dom = DominanceTree::build(&cfg);

        // 0 dominates all
        assert!(dom.dominates(0, 0));
        assert!(dom.dominates(0, 1));
        assert!(dom.dominates(0, 2));

        // 1 dominates 1 and 2
        assert!(dom.dominates(1, 1));
        assert!(dom.dominates(1, 2));
        assert!(!dom.dominates(1, 0));

        // 2 only dominates itself
        assert!(dom.dominates(2, 2));
        assert!(!dom.dominates(2, 0));
        assert!(!dom.dominates(2, 1));
    }

    #[test]
    fn test_dominance_diamond() {
        // Diamond CFG: 0 -> 1, 2 -> 3
        let mut cfg = ControlFlowGraph::new();
        cfg.add_block(); // 0 (entry)
        cfg.add_block(); // 1 (left)
        cfg.add_block(); // 2 (right)
        cfg.add_block(); // 3 (merge)
        cfg.add_edge(0, 1);
        cfg.add_edge(0, 2);
        cfg.add_edge(1, 3);
        cfg.add_edge(2, 3);

        let dom = DominanceTree::build(&cfg);

        // 0 dominates all
        assert!(dom.dominates(0, 0));
        assert!(dom.dominates(0, 1));
        assert!(dom.dominates(0, 2));
        assert!(dom.dominates(0, 3));

        // 1 does not dominate 3 (can reach via 0->2->3)
        assert!(!dom.dominates(1, 3));

        // 2 does not dominate 3 (can reach via 0->1->3)
        assert!(!dom.dominates(2, 3));

        // 3 only dominates itself
        assert!(dom.dominates(3, 3));
        assert!(!dom.dominates(3, 0));
    }

    #[test]
    fn test_idom_diamond() {
        // Diamond CFG
        let mut cfg = ControlFlowGraph::new();
        cfg.add_block(); // 0
        cfg.add_block(); // 1
        cfg.add_block(); // 2
        cfg.add_block(); // 3
        cfg.add_edge(0, 1);
        cfg.add_edge(0, 2);
        cfg.add_edge(1, 3);
        cfg.add_edge(2, 3);

        let dom = DominanceTree::build(&cfg);

        // Immediate dominators
        assert_eq!(dom.idom(0), Some(0)); // Entry
        assert_eq!(dom.idom(1), Some(0));
        assert_eq!(dom.idom(2), Some(0));
        assert_eq!(dom.idom(3), Some(0)); // Both paths from 0
    }

    #[test]
    fn test_lca() {
        // CFG: 0 -> 1 -> 2 -> 3
        //          \-> 4 -> 5
        let mut cfg = ControlFlowGraph::new();
        for _ in 0..6 {
            cfg.add_block();
        }
        cfg.add_edge(0, 1);
        cfg.add_edge(1, 2);
        cfg.add_edge(2, 3);
        cfg.add_edge(1, 4);
        cfg.add_edge(4, 5);

        let dom = DominanceTree::build(&cfg);

        // LCA of nodes in same branch
        assert_eq!(dom.lca(2, 3), Some(2));
        assert_eq!(dom.lca(3, 2), Some(2));

        // LCA of nodes in different branches
        assert_eq!(dom.lca(3, 5), Some(1));

        // LCA with entry
        assert_eq!(dom.lca(0, 5), Some(0));
    }

    #[test]
    fn test_dominated_by() {
        // Linear: 0 -> 1 -> 2
        let mut cfg = ControlFlowGraph::new();
        cfg.add_block();
        cfg.add_block();
        cfg.add_block();
        cfg.add_edge(0, 1);
        cfg.add_edge(1, 2);

        let dom = DominanceTree::build(&cfg);

        let dominated_by_0 = dom.dominated_by(0);
        assert_eq!(dominated_by_0.len(), 3);
        assert!(dominated_by_0.contains(&0));
        assert!(dominated_by_0.contains(&1));
        assert!(dominated_by_0.contains(&2));

        let dominated_by_1 = dom.dominated_by(1);
        assert_eq!(dominated_by_1.len(), 2);
        assert!(dominated_by_1.contains(&1));
        assert!(dominated_by_1.contains(&2));
    }
}
